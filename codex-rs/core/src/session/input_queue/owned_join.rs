use super::InputQueue;
use super::MailboxProvenance;
use crate::agent::control::ExactJoinTurnTerminal;
use crate::agent::control::RetainedJoinDeliveryState;
use crate::agent::status::is_final;
use crate::state::TurnState;
use codex_protocol::protocol::AgentStatus;
use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::watch;

mod continuation;
mod delivery;

pub(super) use self::delivery::mailbox_join_provenance;
pub(super) use self::delivery::validated_mailbox_join_provenance;

/// A private, one-shot completion token owned by the parent input queue.
#[derive(Clone, Debug)]
pub(crate) struct OwnedObligation {
    generation: u64,
    consumed: Arc<AtomicBool>,
}

impl OwnedObligation {
    pub(crate) fn new(generation: u64) -> Self {
        Self {
            generation,
            consumed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn consume_if_current(&self, generation: u64) -> bool {
        self.generation == generation
            && self
                .consumed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
}

#[derive(Default)]
pub(super) struct JoinObligationStore {
    pub(in crate::session::input_queue) next_generation: u64,
    pub(in crate::session::input_queue) obligations: Vec<JoinObligation>,
    pub(in crate::session::input_queue) continuations: Vec<JoinContinuationLease>,
}

pub(in crate::session::input_queue) struct JoinObligation {
    pub(in crate::session::input_queue) parent_thread_id: codex_protocol::ThreadId,
    pub(in crate::session::input_queue) parent_turn_id: String,
    pub(in crate::session::input_queue) targets:
        std::collections::HashMap<codex_protocol::ThreadId, (Arc<dyn Any + Send + Sync>, String)>,
    pub(in crate::session::input_queue) results:
        std::collections::HashMap<codex_protocol::ThreadId, AgentStatus>,
    pub(in crate::session::input_queue) ready: bool,
    pub(in crate::session::input_queue) trigger_sent: bool,
    pub(in crate::session::input_queue) dispatch_complete: bool,
    pub(in crate::session::input_queue) trigger_turn: bool,
    pub(in crate::session::input_queue) retained_targets:
        std::collections::HashSet<codex_protocol::ThreadId>,
    pub(in crate::session::input_queue) retained_delivery_predecessor_turn_id: Option<String>,
    pub(in crate::session::input_queue) token: OwnedObligation,
}

pub(in crate::session::input_queue) struct JoinContinuationLease {
    pub(in crate::session::input_queue) parent_thread_id: codex_protocol::ThreadId,
    pub(in crate::session::input_queue) generation: u64,
    pub(in crate::session::input_queue) predecessor_turn_id: String,
    pub(in crate::session::input_queue) successor_turn_id: String,
    pub(in crate::session::input_queue) successor_turn_state: Arc<Mutex<TurnState>>,
    pub(in crate::session::input_queue) terminal_status: Option<AgentStatus>,
}

pub(crate) struct JoinCompletion {
    pub(crate) parent_turn_id: String,
    pub(crate) generation: u64,
    pub(crate) outcomes: Vec<(codex_protocol::ThreadId, AgentStatus)>,
    pub(crate) trigger_turn: bool,
}

pub(crate) struct RetainedJoinResolution {
    pub(crate) completion: Option<JoinCompletion>,
    pub(crate) parent_turn_id: String,
}

impl InputQueue {
    #[cfg(test)]
    pub(crate) async fn set_active_delivery_ack_barrier(
        &self,
        reached: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) {
        *self.active_delivery_ack_barrier.lock().await = Some((reached, release));
    }

    pub(crate) async fn register_join_obligation(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        parent_turn_id: String,
        targets: std::collections::HashMap<
            codex_protocol::ThreadId,
            (Arc<dyn Any + Send + Sync>, String),
        >,
    ) -> Option<OwnedObligation> {
        self.register_obligation(parent_thread_id, parent_turn_id, targets, true)
            .await
    }

    pub(crate) async fn register_wait_obligation(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        parent_turn_id: String,
        targets: std::collections::HashMap<
            codex_protocol::ThreadId,
            (Arc<dyn Any + Send + Sync>, String),
        >,
    ) -> Option<OwnedObligation> {
        self.register_obligation(parent_thread_id, parent_turn_id, targets, false)
            .await
    }

    async fn register_obligation(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        parent_turn_id: String,
        targets: std::collections::HashMap<
            codex_protocol::ThreadId,
            (Arc<dyn Any + Send + Sync>, String),
        >,
        trigger_turn: bool,
    ) -> Option<OwnedObligation> {
        let mut store = self.join_obligations.lock().await;
        if !trigger_turn
            && let Some(obligation) = store.obligations.iter().find(|obligation| {
                obligation.parent_thread_id == parent_thread_id
                    && obligation.parent_turn_id == parent_turn_id
                    && same_target_bindings(&obligation.targets, &targets)
            })
        {
            return Some(obligation.token.clone());
        }
        if store.obligations.iter().any(|obligation| {
            obligation.parent_thread_id == parent_thread_id
                && obligation.parent_turn_id == parent_turn_id
                && obligation.trigger_turn == trigger_turn
        }) {
            return None;
        }
        store.next_generation = store.next_generation.saturating_add(1);
        let token = OwnedObligation::new(store.next_generation);
        let result = token.clone();
        store.obligations.push(JoinObligation {
            parent_thread_id,
            parent_turn_id,
            targets,
            results: Default::default(),
            ready: false,
            trigger_sent: false,
            dispatch_complete: false,
            trigger_turn,
            retained_targets: Default::default(),
            retained_delivery_predecessor_turn_id: None,
            token,
        });
        Some(result)
    }

    pub(crate) fn subscribe_join_continuations(&self) -> watch::Receiver<u64> {
        self.join_continuation_tx.subscribe()
    }

    pub(crate) fn subscribe_exact_join_terminals(
        &self,
    ) -> broadcast::Receiver<ExactJoinTurnTerminal> {
        self.exact_join_terminal_tx.subscribe()
    }

    pub(crate) async fn exact_join_terminal_status(
        &self,
        turn_id: &str,
        turn_state: &Arc<Mutex<TurnState>>,
    ) -> Option<AgentStatus> {
        let mut terminals = self.exact_join_terminals.lock().await;
        terminals.retain(|terminal| terminal.turn_state.strong_count() > 0);
        terminals
            .iter()
            .rev()
            .find_map(|terminal| terminal.status_for(turn_id, turn_state))
    }

    pub(crate) async fn has_owned_join_state(&self) -> bool {
        let store = self.join_obligations.lock().await;
        !store.obligations.is_empty() || !store.continuations.is_empty()
    }

    pub(crate) async fn join_owner_turn_for_generation(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        generation: u64,
    ) -> Result<Option<String>, ()> {
        let store = self.join_obligations.lock().await;
        let mut matching = store.obligations.iter().filter(|obligation| {
            obligation.parent_thread_id == parent_thread_id
                && obligation.token.generation() == generation
        });
        let first = matching.next();
        if matching.next().is_some() {
            return Err(());
        }
        Ok(first.map(|obligation| obligation.parent_turn_id.clone()))
    }

    pub(crate) async fn retained_join_delivery_state(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        generation: u64,
        child_thread_id: codex_protocol::ThreadId,
    ) -> Result<Option<RetainedJoinDeliveryState>, ()> {
        let store = self.join_obligations.lock().await;
        let mut matching = store.obligations.iter().filter(|obligation| {
            obligation.parent_thread_id == parent_thread_id
                && obligation.token.generation() == generation
        });
        let first = matching.next();
        if matching.next().is_some() {
            return Err(());
        }
        Ok(first.map(|obligation| RetainedJoinDeliveryState {
            owner_turn_id: obligation.parent_turn_id.clone(),
            consumed_for_target: obligation.retained_targets.contains(&child_thread_id)
                && !obligation.results.contains_key(&child_thread_id)
                && obligation.retained_delivery_predecessor_turn_id.is_none(),
        }))
    }

    pub(crate) async fn join_continuation_for_predecessor(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        predecessor_turn_id: &str,
    ) -> Result<
        Option<(
            u64,
            Option<(String, Arc<Mutex<TurnState>>, Option<AgentStatus>)>,
        )>,
        (),
    > {
        let store = self.join_obligations.lock().await;
        let mut bound = store.continuations.iter().filter(|continuation| {
            continuation.parent_thread_id == parent_thread_id
                && continuation.predecessor_turn_id == predecessor_turn_id
        });
        let first_bound = bound.next();
        if bound.next().is_some() {
            return Err(());
        }
        let mut pending = store.obligations.iter().filter(|obligation| {
            obligation.trigger_turn
                && obligation.parent_thread_id == parent_thread_id
                && obligation.parent_turn_id == predecessor_turn_id
        });
        let first_pending = pending.next();
        if pending.next().is_some() || (first_bound.is_some() && first_pending.is_some()) {
            return Err(());
        }
        if let Some(continuation) = first_bound {
            return Ok(Some((
                continuation.generation,
                Some((
                    continuation.successor_turn_id.clone(),
                    Arc::clone(&continuation.successor_turn_state),
                    continuation.terminal_status.clone(),
                )),
            )));
        }
        Ok(first_pending.map(|obligation| (obligation.token.generation(), None)))
    }

    pub(crate) async fn resolve_join_target(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        obligation_generation: u64,
        child_thread_id: codex_protocol::ThreadId,
        child_incarnation: &Arc<dyn Any + Send + Sync>,
        child_turn_id: &str,
        status: AgentStatus,
    ) -> Option<JoinCompletion> {
        self.resolve_join_target_inner(
            parent_thread_id,
            obligation_generation,
            child_thread_id,
            child_incarnation,
            child_turn_id,
            status,
            false,
        )
        .await?
        .completion
    }

    pub(crate) async fn resolve_join_target_retaining_successor(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        obligation_generation: u64,
        child_thread_id: codex_protocol::ThreadId,
        child_incarnation: &Arc<dyn Any + Send + Sync>,
        child_turn_id: &str,
        status: AgentStatus,
    ) -> Option<RetainedJoinResolution> {
        self.resolve_join_target_inner(
            parent_thread_id,
            obligation_generation,
            child_thread_id,
            child_incarnation,
            child_turn_id,
            status,
            true,
        )
        .await
    }

    async fn resolve_join_target_inner(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        obligation_generation: u64,
        child_thread_id: codex_protocol::ThreadId,
        child_incarnation: &Arc<dyn Any + Send + Sync>,
        child_turn_id: &str,
        status: AgentStatus,
        retain_successor: bool,
    ) -> Option<RetainedJoinResolution> {
        if !is_final(&status) || (retain_successor && !matches!(status, AgentStatus::Completed(_)))
        {
            return None;
        }
        let mut store = self.join_obligations.lock().await;
        let index = store.obligations.iter().position(|obligation| {
            obligation.parent_thread_id == parent_thread_id
                && obligation.token.generation() == obligation_generation
                && obligation.targets.contains_key(&child_thread_id)
        })?;
        let Some((expected_incarnation, expected_turn_id)) =
            store.obligations[index].targets.get(&child_thread_id)
        else {
            return None;
        };
        if expected_turn_id != child_turn_id
            || !Arc::ptr_eq(expected_incarnation, child_incarnation)
        {
            return None;
        }
        let obligation = &mut store.obligations[index];
        if retain_successor && !obligation.trigger_turn {
            return None;
        }
        if retain_successor {
            obligation.retained_targets.insert(child_thread_id);
        }
        obligation.results.entry(child_thread_id).or_insert(status);
        let parent_turn_id = obligation.parent_turn_id.clone();
        if obligation.results.len() != obligation.targets.len() {
            return Some(RetainedJoinResolution {
                completion: None,
                parent_turn_id,
            });
        }
        if !obligation.retained_targets.is_empty()
            && obligation.retained_delivery_predecessor_turn_id.is_some()
        {
            return Some(RetainedJoinResolution {
                completion: None,
                parent_turn_id,
            });
        }
        obligation.ready = true;
        let trigger_turn = obligation.trigger_turn;
        if obligation.trigger_sent {
            return Some(RetainedJoinResolution {
                completion: None,
                parent_turn_id,
            });
        }
        obligation.trigger_sent = true;
        if trigger_turn && !obligation.retained_targets.is_empty() {
            obligation.retained_delivery_predecessor_turn_id = Some(parent_turn_id.clone());
        }
        let mut outcomes = obligation
            .results
            .iter()
            .map(|(thread_id, status)| (*thread_id, status.clone()))
            .collect::<Vec<_>>();
        outcomes.sort_by_key(|(thread_id, _)| thread_id.to_string());
        Some(RetainedJoinResolution {
            completion: Some(JoinCompletion {
                parent_turn_id: parent_turn_id.clone(),
                generation: obligation.token.generation(),
                outcomes,
                trigger_turn,
            }),
            parent_turn_id,
        })
    }
}

fn same_target_bindings(
    left: &std::collections::HashMap<
        codex_protocol::ThreadId,
        (Arc<dyn Any + Send + Sync>, String),
    >,
    right: &std::collections::HashMap<
        codex_protocol::ThreadId,
        (Arc<dyn Any + Send + Sync>, String),
    >,
) -> bool {
    left.len() == right.len()
        && left.iter().all(|(thread_id, (incarnation, turn_id))| {
            right
                .get(thread_id)
                .is_some_and(|(other_incarnation, other_turn_id)| {
                    Arc::ptr_eq(incarnation, other_incarnation) && turn_id == other_turn_id
                })
        })
}
