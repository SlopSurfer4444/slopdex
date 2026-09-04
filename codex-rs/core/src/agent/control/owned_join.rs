use super::AgentControl;
use crate::TurnStartOptions;
use crate::agent::AgentStatus;
use crate::agent_communication::AgentCommunicationContext;
use crate::agent_communication::AgentCommunicationKind;
use crate::state::TurnState;
use crate::thread_manager::ThreadManagerState;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::InterAgentCommunication;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::Mutex;

#[derive(Clone)]
pub(crate) struct ExactJoinTurnTerminal {
    pub(crate) turn_id: String,
    pub(crate) turn_state: Weak<Mutex<TurnState>>,
    pub(crate) status: AgentStatus,
}

pub(crate) struct RetainedJoinDeliveryState {
    pub(crate) owner_turn_id: String,
    pub(crate) consumed_for_target: bool,
}

impl ExactJoinTurnTerminal {
    pub(crate) fn status_for(
        &self,
        turn_id: &str,
        turn_state: &Arc<Mutex<TurnState>>,
    ) -> Option<AgentStatus> {
        let recorded_turn_state = self.turn_state.upgrade()?;
        (self.turn_id == turn_id && Arc::ptr_eq(&recorded_turn_state, turn_state))
            .then_some(self.status.clone())
    }
}

#[cfg(test)]
impl AgentControl {
    pub(crate) async fn set_exact_join_observer_receive_barrier(
        &self,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) {
        *self.exact_join_observer_receive_barrier.lock().await = Some((entered, release));
    }
}

impl AgentControl {
    pub(crate) async fn register_join_obligation(
        &self,
        parent_thread_id: ThreadId,
        parent_turn_id: String,
        child_thread_ids: Vec<ThreadId>,
    ) -> CodexResult<bool> {
        self.register_obligation(parent_thread_id, parent_turn_id, child_thread_ids, true)
            .await
    }

    pub(crate) async fn register_wait_obligation(
        &self,
        parent_thread_id: ThreadId,
        parent_turn_id: String,
        child_thread_ids: Vec<ThreadId>,
    ) -> CodexResult<bool> {
        self.register_obligation(parent_thread_id, parent_turn_id, child_thread_ids, false)
            .await
    }

    async fn register_obligation(
        &self,
        parent_thread_id: ThreadId,
        parent_turn_id: String,
        child_thread_ids: Vec<ThreadId>,
        trigger_turn: bool,
    ) -> CodexResult<bool> {
        let state = self.upgrade()?;
        let parent_thread = state.get_thread(parent_thread_id).await?;
        let Some(parent_path) = self
            .get_agent_metadata(parent_thread_id)
            .and_then(|metadata| metadata.agent_path)
        else {
            return Ok(false);
        };
        let mut targets = HashMap::with_capacity(child_thread_ids.len());
        let mut observers = Vec::with_capacity(child_thread_ids.len());
        for child_thread_id in child_thread_ids {
            let child_thread = state.get_thread(child_thread_id).await?;
            let exact_terminal_rx = child_thread
                .session
                .input_queue
                .subscribe_exact_join_terminals();
            let child_turn = {
                let active_turn = child_thread.session.active_turn.lock().await;
                active_turn.as_ref().and_then(|turn| {
                    turn.task.as_ref().map(|task| {
                        (
                            task.turn_context.sub_id.clone(),
                            Arc::clone(&turn.turn_state),
                        )
                    })
                })
            };
            let Some((child_turn_id, child_turn_state)) = child_turn else {
                return Ok(false);
            };
            let Some(child_path) = self
                .get_agent_metadata(child_thread_id)
                .and_then(|metadata| metadata.agent_path)
            else {
                return Ok(false);
            };
            let child_identity: Arc<dyn std::any::Any + Send + Sync> = child_thread.clone();
            targets.insert(child_thread_id, (child_identity, child_turn_id.clone()));
            observers.push((
                child_thread_id,
                child_thread,
                child_turn_id,
                child_turn_state,
                child_path,
                exact_terminal_rx,
            ));
        }
        let obligation = if trigger_turn {
            parent_thread
                .session
                .input_queue
                .register_join_obligation(parent_thread_id, parent_turn_id, targets)
                .await
        } else {
            parent_thread
                .session
                .input_queue
                .register_wait_obligation(parent_thread_id, parent_turn_id, targets)
                .await
        };
        let Some(obligation) = obligation else {
            return Ok(false);
        };
        for (
            child_thread_id,
            child_thread,
            child_turn_id,
            child_turn_state,
            child_path,
            exact_terminal_rx,
        ) in observers
        {
            self.spawn_join_observer(
                Arc::clone(&parent_thread),
                child_thread_id,
                child_thread,
                child_turn_id,
                child_turn_state,
                obligation.generation(),
                parent_path.clone(),
                child_path,
                exact_terminal_rx,
            );
        }
        Ok(true)
    }

    pub(crate) async fn consume_ready_join_obligation_for_targets(
        &self,
        parent_thread_id: ThreadId,
        parent_turn_id: &str,
        child_thread_ids: &[ThreadId],
    ) -> CodexResult<bool> {
        let state = self.upgrade()?;
        let parent_thread = state.get_thread(parent_thread_id).await?;
        let current_targets = self.current_target_bindings(&state, child_thread_ids).await;
        Ok(parent_thread
            .session
            .input_queue
            .consume_ready_join_obligation_for_targets(
                parent_thread_id,
                parent_turn_id,
                child_thread_ids,
                &current_targets,
            )
            .await)
    }

    pub(crate) async fn consume_ready_join_obligation_for_targets_with_outcomes(
        &self,
        parent_thread_id: ThreadId,
        parent_turn_id: &str,
        child_thread_ids: &[ThreadId],
    ) -> CodexResult<Option<Vec<(ThreadId, AgentStatus)>>> {
        let state = self.upgrade()?;
        let parent_thread = state.get_thread(parent_thread_id).await?;
        let current_targets = self.current_target_bindings(&state, child_thread_ids).await;
        Ok(parent_thread
            .session
            .input_queue
            .consume_ready_join_obligation_for_targets_with_outcomes(
                parent_thread_id,
                parent_turn_id,
                child_thread_ids,
                &current_targets,
            )
            .await)
    }

    async fn current_target_bindings(
        &self,
        state: &Arc<ThreadManagerState>,
        child_thread_ids: &[ThreadId],
    ) -> HashMap<ThreadId, (Arc<dyn std::any::Any + Send + Sync>, String)> {
        let mut bindings = HashMap::with_capacity(child_thread_ids.len());
        for child_thread_id in child_thread_ids {
            let Ok(child_thread) = state.get_thread(*child_thread_id).await else {
                continue;
            };
            let child_turn_id = child_thread
                .session
                .active_turn
                .lock()
                .await
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .map(|task| task.turn_context.sub_id.clone())
                .unwrap_or_default();
            let child_identity: Arc<dyn std::any::Any + Send + Sync> = child_thread;
            bindings.insert(*child_thread_id, (child_identity, child_turn_id));
        }
        bindings
    }

    pub(super) async fn dispatch_join_completion(
        &self,
        parent_thread: Arc<crate::codex_thread::CodexThread>,
        child_thread_id: ThreadId,
        parent_path: AgentPath,
        child_path: AgentPath,
        parent_turn_id: String,
        generation: u64,
        outcomes: Vec<(ThreadId, AgentStatus)>,
        trigger_turn: bool,
    ) {
        let parent_thread_id = parent_thread.session.thread_id;
        let Ok(state) = self.upgrade() else {
            return;
        };
        let Ok(current_parent_thread) = state.get_thread(parent_thread_id).await else {
            return;
        };
        if !Arc::ptr_eq(&current_parent_thread, &parent_thread) {
            return;
        }
        if !trigger_turn {
            parent_thread
                .session
                .input_queue
                .finish_join_dispatch(generation)
                .await;
            parent_thread
                .session
                .input_queue
                .notify_wait_obligation_ready();
            return;
        }

        let outcomes = outcomes
            .iter()
            .map(|(thread_id, status)| format!("{thread_id}: {status:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let communication = InterAgentCommunication::new(
            child_path,
            parent_path,
            Vec::new(),
            format!(
                "All requested direct children reached terminal states for parent turn {parent_turn_id}: {outcomes}"
            ),
            /*trigger_turn*/ true,
        );
        let mut start_options = TurnStartOptions::default();
        start_options.parent_turn_id = Some(parent_turn_id.clone());
        let release_rx = self.agent_execution_limiter.subscribe_release();
        let release_generation = *release_rx.borrow();
        parent_thread
            .session
            .input_queue
            .mark_join_trigger(&parent_turn_id, generation)
            .await;
        let context =
            AgentCommunicationContext::new(AgentCommunicationKind::Result, child_thread_id);
        let Ok(state) = self.upgrade() else {
            return;
        };
        let Ok(current_parent_thread) = state.get_thread(parent_thread_id).await else {
            return;
        };
        if !Arc::ptr_eq(&current_parent_thread, &parent_thread) {
            parent_thread
                .session
                .input_queue
                .clear_join_trigger(&parent_turn_id, generation)
                .await;
            return;
        }
        let send_result = self
            .send_inter_agent_communication(
                parent_thread_id,
                communication.clone(),
                context,
                start_options.clone(),
            )
            .await;
        if send_result
            .as_ref()
            .is_err_and(|err| matches!(err.details(), CodexErrorDetails::AgentLimitReached { .. }))
        {
            self.retain_capacity_ready_lease(
                Arc::clone(&parent_thread),
                parent_turn_id.clone(),
                generation,
                communication,
                start_options,
                release_rx,
                release_generation,
            )
            .await;
        } else if send_result.is_err() {
            parent_thread
                .session
                .input_queue
                .clear_join_trigger(&parent_turn_id, generation)
                .await;
        }
        parent_thread
            .session
            .input_queue
            .finish_join_dispatch(generation)
            .await;
    }
}
