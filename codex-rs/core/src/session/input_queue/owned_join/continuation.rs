use super::super::MailboxProvenance;
use super::InputQueue;
use crate::agent::control::ExactJoinTurnTerminal;
use crate::agent::status::is_final;
use crate::state::TurnState;
use codex_protocol::protocol::AgentStatus;
use std::any::Any;
use std::sync::Arc;
use tokio::sync::Mutex;

impl InputQueue {
    pub(crate) async fn rebind_join_target_to_successor(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        obligation_generation: u64,
        child_thread_id: codex_protocol::ThreadId,
        child_incarnation: &Arc<dyn Any + Send + Sync>,
        predecessor_turn_id: &str,
        successor_turn_id: &str,
    ) -> bool {
        if predecessor_turn_id.is_empty()
            || successor_turn_id.is_empty()
            || predecessor_turn_id == successor_turn_id
        {
            return false;
        }
        let mut store = self.join_obligations.lock().await;
        let Some(obligation) = store.obligations.iter_mut().find(|obligation| {
            obligation.parent_thread_id == parent_thread_id
                && obligation.token.generation() == obligation_generation
                && !obligation.ready
                && !obligation.results.contains_key(&child_thread_id)
                && obligation.retained_delivery_predecessor_turn_id.is_none()
                && obligation.retained_targets.contains(&child_thread_id)
        }) else {
            return false;
        };
        let Some((expected_incarnation, expected_turn_id)) =
            obligation.targets.get_mut(&child_thread_id)
        else {
            return false;
        };
        if expected_turn_id != predecessor_turn_id
            || !Arc::ptr_eq(expected_incarnation, child_incarnation)
        {
            return false;
        }
        *expected_turn_id = successor_turn_id.to_string();
        obligation.retained_targets.remove(&child_thread_id);
        true
    }

    pub(crate) async fn retire_join_target_successor_retention(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        obligation_generation: u64,
        child_thread_id: codex_protocol::ThreadId,
        child_incarnation: &Arc<dyn Any + Send + Sync>,
        predecessor_turn_id: &str,
    ) -> bool {
        let mut store = self.join_obligations.lock().await;
        let Some(obligation) = store.obligations.iter_mut().find(|obligation| {
            obligation.parent_thread_id == parent_thread_id
                && obligation.token.generation() == obligation_generation
                && !obligation.ready
                && !obligation.results.contains_key(&child_thread_id)
                && obligation.retained_delivery_predecessor_turn_id.is_none()
                && obligation.retained_targets.contains(&child_thread_id)
        }) else {
            return false;
        };
        let Some((expected_incarnation, expected_turn_id)) =
            obligation.targets.get(&child_thread_id)
        else {
            return false;
        };
        if expected_turn_id != predecessor_turn_id
            || !Arc::ptr_eq(expected_incarnation, child_incarnation)
        {
            return false;
        }
        obligation.retained_targets.remove(&child_thread_id);
        true
    }

    pub(crate) async fn finish_join_continuation(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        generation: u64,
        predecessor_turn_id: &str,
        successor_turn_id: &str,
        successor_turn_state: &Arc<Mutex<TurnState>>,
    ) {
        let mut store = self.join_obligations.lock().await;
        let before = store.continuations.len();
        store.continuations.retain(|continuation| {
            continuation.parent_thread_id != parent_thread_id
                || continuation.generation != generation
                || continuation.predecessor_turn_id != predecessor_turn_id
                || continuation.successor_turn_id != successor_turn_id
                || !Arc::ptr_eq(&continuation.successor_turn_state, successor_turn_state)
        });
        if store.continuations.len() != before {
            self.bump_join_continuation_revision();
        }
    }

    pub(crate) async fn finish_root_join_continuations(
        &self,
        root_thread_id: codex_protocol::ThreadId,
        terminal_turn_id: &str,
        terminal_turn_state: &Arc<Mutex<TurnState>>,
    ) {
        let mut store = self.join_obligations.lock().await;
        let before = store.continuations.len();
        store.continuations.retain(|continuation| {
            continuation.parent_thread_id != root_thread_id
                || continuation.successor_turn_id != terminal_turn_id
                || !Arc::ptr_eq(&continuation.successor_turn_state, terminal_turn_state)
        });
        if store.continuations.len() != before {
            self.bump_join_continuation_revision();
        }
    }

    pub(crate) async fn record_nested_join_turn_terminal(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        turn_id: &str,
        turn_state: &Arc<Mutex<TurnState>>,
        status: AgentStatus,
    ) {
        if !is_final(&status) {
            return;
        }

        // Preserve the established mailbox -> join lock order. A non-successful predecessor
        // cannot transfer its live nested generation into later pending work.
        let mut pending_mails = self.mailbox_pending_mails.lock().await;
        let mut store = self.join_obligations.lock().await;
        let mut changed = false;
        for continuation in &mut store.continuations {
            if continuation.parent_thread_id == parent_thread_id
                && continuation.successor_turn_id == turn_id
                && Arc::ptr_eq(&continuation.successor_turn_state, turn_state)
                && continuation.terminal_status.is_none()
            {
                continuation.terminal_status = Some(status.clone());
                changed = true;
            }
        }

        if !matches!(status, AgentStatus::Completed(_)) {
            let retired_generations = store
                .obligations
                .iter()
                .filter(|obligation| {
                    obligation.trigger_turn
                        && obligation.parent_thread_id == parent_thread_id
                        && obligation.parent_turn_id == turn_id
                })
                .map(|obligation| obligation.token.generation())
                .collect::<Vec<_>>();
            if !retired_generations.is_empty() {
                store.obligations.retain(|obligation| {
                    !retired_generations.contains(&obligation.token.generation())
                });
                pending_mails.retain(|mail| {
                    !matches!(
                        mail.provenance,
                        MailboxProvenance::JoinAggregate { generation }
                            if retired_generations.contains(&generation)
                    )
                });
                changed = true;
            }
        }
        drop(store);
        drop(pending_mails);
        if changed {
            self.bump_join_continuation_revision();
        }
        let exact_terminal = ExactJoinTurnTerminal {
            turn_id: turn_id.to_string(),
            turn_state: Arc::downgrade(turn_state),
            status,
        };
        {
            let mut terminals = self.exact_join_terminals.lock().await;
            terminals.retain(|terminal| terminal.turn_state.strong_count() > 0);
            if let Some(existing) = terminals.iter_mut().find(|terminal| {
                terminal.turn_id == exact_terminal.turn_id
                    && terminal
                        .turn_state
                        .upgrade()
                        .is_some_and(|state| Arc::ptr_eq(&state, turn_state))
            }) {
                *existing = exact_terminal.clone();
            } else {
                terminals.push(exact_terminal.clone());
            }
        }
        let _ = self.exact_join_terminal_tx.send(exact_terminal);
    }

    pub(in crate::session::input_queue) fn bump_join_continuation_revision(&self) {
        self.join_continuation_tx
            .send_modify(|revision| *revision = revision.saturating_add(1));
    }
}
