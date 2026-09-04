use super::*;
use codex_protocol::error::CodexErrorDetails;
use codex_thread_store::PersistContext;

#[derive(Clone, Copy)]
enum RemovalMode {
    PreserveSpawnEdge,
    CloseSpawnEdge,
}

impl AgentControl {
    pub(crate) async fn shutdown_live_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        self.shutdown_live_agent_with_removal_mode(agent_id, RemovalMode::PreserveSpawnEdge)
            .await
    }

    async fn shutdown_live_agent_with_removal_mode(
        &self,
        agent_id: ThreadId,
        removal_mode: RemovalMode,
    ) -> CodexResult<String> {
        let state = self.upgrade()?;
        let expected_thread = state.get_thread(agent_id).await.ok();
        let expected_incarnation = match expected_thread.as_ref() {
            Some(thread) => state.thread_incarnation_for(agent_id, thread).await,
            None => None,
        };
        let result = if let Some(thread) = expected_thread.as_ref() {
            thread
                .session
                .ensure_rollout_materialized(PersistContext::Standard)
                .await;
            thread.session.flush_rollout().await?;
            let result = if matches!(thread.agent_status().await, AgentStatus::Shutdown) {
                Ok(String::new())
            } else {
                state
                    .send_op(
                        agent_id,
                        Op::Shutdown {},
                        /*parent_turn_id*/ None,
                        /*root_turn_id*/ None,
                    )
                    .await
            };
            thread.wait_until_terminated().await;
            result
        } else {
            state
                .send_op(
                    agent_id,
                    Op::Shutdown {},
                    /*parent_turn_id*/ None,
                    /*root_turn_id*/ None,
                )
                .await
        };
        #[cfg(test)]
        if matches!(removal_mode, RemovalMode::CloseSpawnEdge) {
            let hook = state
                .close_before_lifecycle_commit_hook
                .lock()
                .await
                .clone();
            if let Some((target, entered, release)) = hook
                && target == agent_id
            {
                entered.wait().await;
                release.notified().await;
            }
        }
        if let Some(expected_thread) = expected_thread.as_ref() {
            #[cfg(test)]
            let close_probe = if matches!(removal_mode, RemovalMode::CloseSpawnEdge) {
                state
                    .close_after_durable_edge_probe
                    .lock()
                    .await
                    .as_ref()
                    .filter(|probe| probe.target == agent_id)
                    .cloned()
            } else {
                None
            };
            match removal_mode {
                RemovalMode::PreserveSpawnEdge => {
                    if state
                        .remove_runtime_if_matches(agent_id, expected_thread)
                        .await
                        .is_some()
                    {
                        self.forget_v2_residency(agent_id);
                        self.state.release_spawned_thread(agent_id);
                    }
                }
                RemovalMode::CloseSpawnEdge => {
                    // Cleanup is an owned lifecycle commit: cancelling the caller
                    // cannot leave a durable Closed edge with live registry state.
                    let control = self.clone();
                    let state = state.clone();
                    let expected_thread = expected_thread.clone();
                    tokio::spawn(async move {
                        if state
                            .close_spawn_edge_and_remove_if_matches(
                                agent_id,
                                &expected_thread,
                                expected_incarnation,
                            )
                            .await?
                            .is_none()
                        {
                            return Err(CodexErr::Fatal(format!(
                                "thread {agent_id} changed generation during explicit close"
                            )));
                        }
                        control.forget_v2_residency(agent_id);
                        control.state.release_spawned_thread(agent_id);
                        #[cfg(test)]
                        if let Some(probe) = close_probe {
                            probe.complete();
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|err| {
                        CodexErr::Fatal(format!("explicit close commit failed: {err}"))
                    })??;
                }
            }
        }
        result
    }

    /// Mark `agent_id` as explicitly closed in persisted spawn-edge state, then shut down the
    /// agent and any live descendants reached from the in-memory tree.
    pub(crate) async fn close_agent(&self, agent_id: ThreadId) -> CodexResult<String> {
        let state = self.upgrade()?;
        let known_agent = self.state.agent_metadata_for_thread(agent_id).is_some();
        let expected_incarnation = state.thread_incarnation(agent_id).await;
        match Box::pin(
            self.shutdown_agent_tree_with_removal_mode(agent_id, RemovalMode::CloseSpawnEdge),
        )
        .await
        {
            Err(err)
                if known_agent && matches!(err.details(), CodexErrorDetails::ThreadNotFound(_)) =>
            {
                let is_root = self
                    .state
                    .agent_metadata_for_thread(agent_id)
                    .and_then(|metadata| metadata.agent_path)
                    .is_some_and(|path| path.is_root());
                if !is_root {
                    let root_thread_id = self
                        .state
                        .agent_id_for_path(&AgentPath::root())
                        .ok_or_else(|| {
                            CodexErr::Fatal("root agent is not registered".to_string())
                        })?;
                    let edge_closed = state
                        .close_stale_spawn_edge(root_thread_id, agent_id, expected_incarnation)
                        .await?;
                    if !edge_closed {
                        return Err(CodexErr::Fatal(format!(
                            "stale thread-spawn edge for {agent_id} was not found under the current session"
                        )));
                    }
                }
                self.forget_v2_residency(agent_id);
                self.state.release_spawned_thread(agent_id);
                Ok(String::new())
            }
            result => result,
        }
    }

    /// Shut down `agent_id` and any live descendants reachable from the in-memory spawn tree.
    pub(crate) async fn shutdown_agent_tree(&self, agent_id: ThreadId) -> CodexResult<String> {
        self.shutdown_agent_tree_with_removal_mode(agent_id, RemovalMode::PreserveSpawnEdge)
            .await
    }

    async fn shutdown_agent_tree_with_removal_mode(
        &self,
        agent_id: ThreadId,
        target_removal_mode: RemovalMode,
    ) -> CodexResult<String> {
        let descendant_ids = self.live_thread_spawn_descendants(agent_id).await?;
        let result = self
            .shutdown_live_agent_with_removal_mode(agent_id, target_removal_mode)
            .await;
        for descendant_id in descendant_ids {
            match self
                .shutdown_live_agent_with_removal_mode(
                    descendant_id,
                    RemovalMode::PreserveSpawnEdge,
                )
                .await
            {
                Ok(_) => {}
                Err(err) if matches!(err.details(), CodexErrorDetails::ThreadNotFound(_)) => {}
                Err(err) => return Err(err),
            }
        }
        result
    }
}
