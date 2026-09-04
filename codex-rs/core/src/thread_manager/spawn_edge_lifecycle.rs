use super::CodexThread;
use super::ThreadManagerState;
use codex_agent_graph_store::ThreadSpawnEdgeCloseOutcome;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Weak;
#[cfg(test)]
use tokio::sync::Mutex;

#[cfg(test)]
pub(crate) struct CloseAfterDurableEdgeTestProbe {
    pub(crate) target: ThreadId,
    pub(crate) entered: Arc<tokio::sync::Barrier>,
    pub(crate) release: Arc<tokio::sync::Notify>,
    completion: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[cfg(test)]
impl CloseAfterDurableEdgeTestProbe {
    pub(crate) fn new(
        target: ThreadId,
        entered: Arc<tokio::sync::Barrier>,
        release: Arc<tokio::sync::Notify>,
    ) -> (Arc<Self>, tokio::sync::oneshot::Receiver<()>) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (
            Arc::new(Self {
                target,
                entered,
                release,
                completion: std::sync::Mutex::new(Some(sender)),
            }),
            receiver,
        )
    }

    pub(crate) fn complete(&self) {
        if let Some(sender) = self
            .completion
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = sender.send(());
        }
    }
}

#[cfg(test)]
pub(crate) type LifecycleCommitTestHook = Arc<
    Mutex<
        Option<(
            ThreadId,
            Arc<tokio::sync::Barrier>,
            Arc<tokio::sync::Notify>,
        )>,
    >,
>;

#[derive(Default)]
pub(super) struct ThreadIncarnations {
    pub(super) next: u64,
    pub(super) latest: HashMap<ThreadId, (u64, Weak<CodexThread>)>,
}

impl ThreadManagerState {
    pub(super) async fn record_thread_incarnation(
        &self,
        thread_id: ThreadId,
        thread: &Arc<CodexThread>,
    ) {
        let mut incarnations = self.thread_incarnations.lock().await;
        incarnations.next = incarnations
            .next
            .checked_add(1)
            .expect("thread incarnation counter exhausted");
        let incarnation = incarnations.next;
        incarnations
            .latest
            .insert(thread_id, (incarnation, Arc::downgrade(thread)));
    }

    pub(crate) async fn thread_incarnation(&self, thread_id: ThreadId) -> Option<u64> {
        self.thread_incarnations
            .lock()
            .await
            .latest
            .get(&thread_id)
            .map(|entry| entry.0)
    }

    pub(crate) async fn thread_incarnation_for(
        &self,
        thread_id: ThreadId,
        expected: &Arc<CodexThread>,
    ) -> Option<u64> {
        self.thread_incarnations
            .lock()
            .await
            .latest
            .get(&thread_id)
            .filter(|(_, thread)| Weak::ptr_eq(thread, &Arc::downgrade(expected)))
            .map(|entry| entry.0)
    }

    pub(crate) async fn persist_open_spawn_edge_if_matches(
        &self,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
        expected: &Arc<CodexThread>,
    ) -> CodexResult<()> {
        let _registration_fence = self.thread_registration_fence.lock().await;
        let registered = self.get_thread(child_thread_id).await?;
        if !Arc::ptr_eq(&registered, expected)
            || self
                .thread_incarnation_for(child_thread_id, expected)
                .await
                .is_none()
        {
            return Err(CodexErr::InvalidRequest(format!(
                "thread {child_thread_id} changed incarnation before its spawn edge could be registered"
            )));
        }
        let store = self.agent_graph_store().ok_or_else(|| {
            CodexErr::Fatal(format!(
                "cannot persist thread-spawn edge {parent_thread_id}->{child_thread_id}: graph store unavailable"
            ))
        })?;
        store
            .upsert_thread_spawn_edge(
                parent_thread_id,
                child_thread_id,
                codex_agent_graph_store::ThreadSpawnEdgeStatus::Open,
            )
            .await
            .map_err(|err| CodexErr::Fatal(format!("failed to persist thread-spawn edge: {err}")))
    }

    /// Close the exact incoming spawn edge and then remove its exact runtime
    /// generation. Persistence failures and parent/child mismatches leave the
    /// runtime registered so cleanup fails closed.
    pub(crate) async fn close_spawn_edge_and_remove_if_matches(
        &self,
        child_thread_id: ThreadId,
        expected: &Arc<CodexThread>,
        expected_incarnation: Option<u64>,
    ) -> CodexResult<Option<Arc<CodexThread>>> {
        let _registration_fence = self.thread_registration_fence.lock().await;
        if expected_incarnation.is_none()
            || self.thread_incarnation_for(child_thread_id, expected).await != expected_incarnation
        {
            return Ok(None);
        }

        if !expected.config_snapshot().await.ephemeral
            && let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id, ..
            }) = &expected.session_source
        {
            let Some(agent_graph_store) = self.agent_graph_store() else {
                return Err(CodexErr::Fatal(format!(
                    "cannot close thread-spawn edge {parent_thread_id}->{child_thread_id}: graph store unavailable"
                )));
            };
            let outcome = agent_graph_store
                .close_open_thread_spawn_edge(*parent_thread_id, child_thread_id)
                .await
                .map_err(|err| {
                    CodexErr::Fatal(format!(
                        "failed to close thread-spawn edge {parent_thread_id}->{child_thread_id}: {err}"
                    ))
                })?;
            if matches!(outcome, ThreadSpawnEdgeCloseOutcome::MismatchOrMissing) {
                return Err(CodexErr::Fatal(format!(
                    "thread-spawn edge {parent_thread_id}->{child_thread_id} was missing or had a mismatched generation"
                )));
            }
        }

        #[cfg(test)]
        let probe = {
            let mut slot = self.close_after_durable_edge_probe.lock().await;
            slot.take().filter(|probe| probe.target == child_thread_id)
        };
        #[cfg(test)]
        if let Some(probe) = probe.as_ref() {
            probe.entered.wait().await;
            probe.release.notified().await;
        }

        let mut threads = self.threads.write().await;
        if threads
            .get(&child_thread_id)
            .is_some_and(|thread| Arc::ptr_eq(thread, expected))
        {
            Ok(threads.remove(&child_thread_id))
        } else {
            // An exact runtime unload raced this explicit close. The durable
            // close above is still the authoritative lifecycle commit.
            Ok(Some(expected.clone()))
        }
    }

    /// Removes only the exact in-memory runtime generation; durable spawn edges remain intact.
    pub(crate) async fn remove_runtime_if_matches(
        &self,
        thread_id: ThreadId,
        expected: &Arc<CodexThread>,
    ) -> Option<Arc<CodexThread>> {
        let _registration_fence = self.thread_registration_fence.lock().await;
        let mut threads = self.threads.write().await;
        if threads
            .get(&thread_id)
            .is_some_and(|thread| Arc::ptr_eq(thread, expected))
        {
            threads.remove(&thread_id)
        } else {
            None
        }
    }

    /// Close an incoming edge for a stale runtime by resolving its parent from
    /// the persisted tree rooted at the current session. This is used only
    /// after the live map has confirmed that no runtime is loaded.
    pub(crate) async fn close_stale_spawn_edge(
        &self,
        root_thread_id: ThreadId,
        child_thread_id: ThreadId,
        expected_incarnation: Option<u64>,
    ) -> CodexResult<bool> {
        let _registration_fence = self.thread_registration_fence.lock().await;
        if self.thread_incarnation(child_thread_id).await != expected_incarnation {
            return Err(CodexErr::Fatal(format!(
                "thread {child_thread_id} changed incarnation during explicit close"
            )));
        }
        let Some(agent_graph_store) = self.agent_graph_store() else {
            return Err(CodexErr::Fatal(format!(
                "cannot resolve stale thread-spawn edge for {child_thread_id}: graph store unavailable"
            )));
        };
        let mut pending = vec![root_thread_id];
        let mut visited = HashSet::new();
        while let Some(parent_thread_id) = pending.pop() {
            if !visited.insert(parent_thread_id) {
                continue;
            }
            for candidate_child_id in agent_graph_store
                .list_thread_spawn_children(parent_thread_id, None)
                .await
                .map_err(|err| {
                    CodexErr::Fatal(format!(
                        "failed to resolve stale thread-spawn edge for {child_thread_id}: {err}"
                    ))
                })?
            {
                if candidate_child_id == child_thread_id {
                    let outcome = agent_graph_store
                        .close_open_thread_spawn_edge(parent_thread_id, child_thread_id)
                        .await
                        .map_err(|err| {
                            CodexErr::Fatal(format!(
                                "failed to close stale thread-spawn edge {parent_thread_id}->{child_thread_id}: {err}"
                            ))
                        })?;
                    if matches!(outcome, ThreadSpawnEdgeCloseOutcome::MismatchOrMissing) {
                        return Err(CodexErr::Fatal(format!(
                            "stale thread-spawn edge {parent_thread_id}->{child_thread_id} was missing or already replaced"
                        )));
                    }
                    return Ok(true);
                }
                pending.push(candidate_child_id);
            }
        }
        Ok(false)
    }
}
