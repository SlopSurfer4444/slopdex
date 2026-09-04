use super::AgentControl;
use super::ExactJoinTurnTerminal;
use crate::agent::AgentStatus;
use crate::state::TurnState;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use std::sync::Arc;
use tokio::sync::Mutex;

impl AgentControl {
    pub(super) fn spawn_join_observer(
        &self,
        parent_thread: Arc<crate::codex_thread::CodexThread>,
        child_thread_id: ThreadId,
        child_thread: Arc<crate::codex_thread::CodexThread>,
        child_turn_id: String,
        child_turn_state: Arc<Mutex<TurnState>>,
        obligation_generation: u64,
        parent_path: AgentPath,
        child_path: AgentPath,
        mut exact_terminal_rx: tokio::sync::broadcast::Receiver<ExactJoinTurnTerminal>,
    ) {
        let control = self.clone();
        tokio::spawn(async move {
            let parent_thread_id = parent_thread.session.thread_id;
            let mut status_rx = child_thread.subscribe_status();
            let child_identity: Arc<dyn std::any::Any + Send + Sync> = child_thread.clone();
            let mut observed_turn_id = child_turn_id;
            let mut observed_turn_state = child_turn_state;
            #[cfg(test)]
            if let Some((entered, release)) = control
                .exact_join_observer_receive_barrier
                .lock()
                .await
                .take()
            {
                entered.notify_one();
                release.notified().await;
            }
            let mut terminal_status = loop {
                let status = status_rx.borrow_and_update().clone();
                if matches!(status, AgentStatus::Shutdown) {
                    break status;
                }
                tokio::select! {
                    changed = status_rx.changed() => {
                        if changed.is_err() {
                            return;
                        }
                    }
                    exact_terminal = exact_terminal_rx.recv() => {
                        match exact_terminal {
                            Ok(exact_terminal) => {
                                if let Some(status) = exact_terminal.status_for(
                                    &observed_turn_id,
                                    &observed_turn_state,
                                ) {
                                    break status;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                if let Some(status) = child_thread
                                    .session
                                    .input_queue
                                    .exact_join_terminal_status(
                                        &observed_turn_id,
                                        &observed_turn_state,
                                    )
                                    .await
                                {
                                    break status;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
            };

            'successors: loop {
                if matches!(terminal_status, AgentStatus::Completed(_)) {
                    let mut continuation_rx = child_thread
                        .session
                        .input_queue
                        .subscribe_join_continuations();
                    let mut parent_ownership_rx = parent_thread
                        .session
                        .input_queue
                        .subscribe_join_continuations();
                    let mut expected_generation = None;
                    let mut retained_delivery_registered = false;
                    let mut terminal_override = None;
                    let successor = loop {
                        let snapshot = child_thread
                            .session
                            .input_queue
                            .join_continuation_for_predecessor(child_thread_id, &observed_turn_id)
                            .await;
                        let Ok(snapshot) = snapshot else {
                            return;
                        };
                        match snapshot {
                            None if expected_generation.is_none() => break None,
                            None => {
                                let status = status_rx.borrow_and_update().clone();
                                if matches!(status, AgentStatus::Shutdown) {
                                    terminal_override = Some(status);
                                    break None;
                                }
                                return;
                            }
                            Some((generation, _)) if !retained_delivery_registered => {
                                if expected_generation
                                    .replace(generation)
                                    .is_some_and(|expected| expected != generation)
                                {
                                    return;
                                }
                                let Some(retained) = parent_thread
                                    .session
                                    .input_queue
                                    .resolve_join_target_retaining_successor(
                                        parent_thread_id,
                                        obligation_generation,
                                        child_thread_id,
                                        &child_identity,
                                        &observed_turn_id,
                                        terminal_status.clone(),
                                    )
                                    .await
                                else {
                                    return;
                                };
                                if let Some(completion) = retained.completion {
                                    control
                                        .dispatch_join_completion(
                                            Arc::clone(&parent_thread),
                                            child_thread_id,
                                            parent_path.clone(),
                                            child_path.clone(),
                                            completion.parent_turn_id,
                                            completion.generation,
                                            completion.outcomes,
                                            completion.trigger_turn,
                                        )
                                        .await;
                                }
                                loop {
                                    let Ok(state) = control.upgrade() else {
                                        return;
                                    };
                                    let Ok(current_parent_thread) =
                                        state.get_thread(parent_thread_id).await
                                    else {
                                        return;
                                    };
                                    if !Arc::ptr_eq(&current_parent_thread, &parent_thread) {
                                        return;
                                    }
                                    match parent_thread
                                        .session
                                        .input_queue
                                        .retained_join_delivery_state(
                                            parent_thread_id,
                                            obligation_generation,
                                            child_thread_id,
                                        )
                                        .await
                                    {
                                        Ok(Some(state))
                                            if state.owner_turn_id != retained.parent_turn_id
                                                || state.consumed_for_target =>
                                        {
                                            break;
                                        }
                                        Ok(Some(_)) => {
                                            if parent_ownership_rx.changed().await.is_err() {
                                                return;
                                            }
                                        }
                                        Ok(None) | Err(()) => return,
                                    }
                                }
                                retained_delivery_registered = true;
                                continue;
                            }
                            Some((generation, None)) => {
                                if expected_generation
                                    .replace(generation)
                                    .is_some_and(|expected| expected != generation)
                                {
                                    return;
                                }
                                tokio::select! {
                                    changed = continuation_rx.changed() => {
                                        if changed.is_err() {
                                            return;
                                        }
                                    }
                                    changed = status_rx.changed() => {
                                        if changed.is_err() {
                                            return;
                                        }
                                        let status = status_rx.borrow_and_update().clone();
                                        if matches!(status, AgentStatus::Shutdown) {
                                            terminal_override = Some(status);
                                        }
                                    }
                                }
                                if terminal_override.is_some() {
                                    break None;
                                }
                            }
                            Some((generation, Some(successor))) => {
                                if expected_generation
                                    .is_some_and(|expected| expected != generation)
                                {
                                    return;
                                }
                                break Some((generation, successor));
                            }
                        }
                    };

                    if let Some(status) = terminal_override {
                        if retained_delivery_registered
                            && !parent_thread
                                .session
                                .input_queue
                                .retire_join_target_successor_retention(
                                    parent_thread_id,
                                    obligation_generation,
                                    child_thread_id,
                                    &child_identity,
                                    &observed_turn_id,
                                )
                                .await
                        {
                            return;
                        }
                        terminal_status = status;
                    } else if let Some((
                        inner_generation,
                        (successor_turn_id, successor_turn_state, mut successor_terminal_status),
                    )) = successor
                    {
                        if successor_turn_id.is_empty() || successor_turn_id == observed_turn_id {
                            return;
                        }
                        if successor_terminal_status.is_none() {
                            let active = child_thread.session.active_turn.lock().await;
                            let exact_successor_is_live =
                                active.as_ref().is_some_and(|active_turn| {
                                    Arc::ptr_eq(&active_turn.turn_state, &successor_turn_state)
                                        && active_turn.task.as_ref().is_some_and(|task| {
                                            task.turn_context.sub_id == successor_turn_id
                                        })
                                });
                            if !exact_successor_is_live {
                                return;
                            }
                        }

                        let Ok(state) = control.upgrade() else {
                            return;
                        };
                        let Ok(current_parent_thread) = state.get_thread(parent_thread_id).await
                        else {
                            return;
                        };
                        if !Arc::ptr_eq(&current_parent_thread, &parent_thread)
                            || !parent_thread
                                .session
                                .input_queue
                                .rebind_join_target_to_successor(
                                    parent_thread_id,
                                    obligation_generation,
                                    child_thread_id,
                                    &child_identity,
                                    &observed_turn_id,
                                    &successor_turn_id,
                                )
                                .await
                        {
                            return;
                        }

                        let predecessor_turn_id = observed_turn_id;
                        observed_turn_id = successor_turn_id;
                        observed_turn_state = successor_turn_state;
                        // Fence the predecessor's terminal watch value. Any later final value is
                        // attributable to the exact successor installed above.
                        let _ = status_rx.borrow_and_update();
                        while successor_terminal_status.is_none() {
                            let snapshot = child_thread
                                .session
                                .input_queue
                                .join_continuation_for_predecessor(
                                    child_thread_id,
                                    &predecessor_turn_id,
                                )
                                .await;
                            let Ok(Some((
                                generation,
                                Some((
                                    current_successor_turn_id,
                                    current_successor_turn_state,
                                    current_terminal_status,
                                )),
                            ))) = snapshot
                            else {
                                return;
                            };
                            if generation != inner_generation
                                || current_successor_turn_id != observed_turn_id
                                || !Arc::ptr_eq(&current_successor_turn_state, &observed_turn_state)
                            {
                                return;
                            }
                            successor_terminal_status = current_terminal_status;
                            if successor_terminal_status.is_some() {
                                break;
                            }

                            tokio::select! {
                                changed = continuation_rx.changed() => {
                                    if changed.is_err() {
                                        return;
                                    }
                                }
                                changed = status_rx.changed() => {
                                    if changed.is_err() {
                                        return;
                                    }
                                    let status = status_rx.borrow_and_update().clone();
                                    if matches!(status, AgentStatus::Shutdown) {
                                        successor_terminal_status = Some(status);
                                    }
                                }
                            }
                        }
                        let Some(status) = successor_terminal_status else {
                            return;
                        };
                        child_thread
                            .session
                            .input_queue
                            .finish_join_continuation(
                                child_thread_id,
                                inner_generation,
                                &predecessor_turn_id,
                                &observed_turn_id,
                                &observed_turn_state,
                            )
                            .await;
                        terminal_status = status;
                        continue 'successors;
                    }
                }
                break;
            }

            let Some(completion) = parent_thread
                .session
                .input_queue
                .resolve_join_target(
                    parent_thread_id,
                    obligation_generation,
                    child_thread_id,
                    &child_identity,
                    &observed_turn_id,
                    terminal_status,
                )
                .await
            else {
                return;
            };
            control
                .dispatch_join_completion(
                    Arc::clone(&parent_thread),
                    child_thread_id,
                    parent_path,
                    child_path,
                    completion.parent_turn_id,
                    completion.generation,
                    completion.outcomes,
                    completion.trigger_turn,
                )
                .await;
        });
    }
}
