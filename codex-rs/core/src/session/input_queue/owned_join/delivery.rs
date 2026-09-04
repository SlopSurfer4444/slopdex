use super::super::InputQueue;
use super::super::InputQueueActivity;
use super::super::MailboxProvenance;
use super::super::PendingMailboxCommunication;
use super::JoinContinuationLease;
use super::JoinObligationStore;
use crate::state::TurnState;
use codex_protocol::protocol::AgentStatus;
use std::any::Any;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;

impl InputQueue {
    pub(in crate::session::input_queue) fn acknowledge_ready_join_delivery_locked(
        store: &mut JoinObligationStore,
        predecessor_turn_id: &str,
        generation: u64,
        successor_turn_id: &str,
        successor_turn_state: Option<&Arc<Mutex<TurnState>>>,
    ) -> Option<bool> {
        let index = store.obligations.iter().position(|obligation| {
            obligation.token.generation() == generation
                && ((obligation.parent_turn_id == predecessor_turn_id && obligation.ready)
                    || obligation.retained_delivery_predecessor_turn_id.as_deref()
                        == Some(predecessor_turn_id))
        })?;
        let retained_delivery = store.obligations[index]
            .retained_delivery_predecessor_turn_id
            .as_deref()
            == Some(predecessor_turn_id);
        if retained_delivery {
            let parent_thread_id = {
                let obligation = &mut store.obligations[index];
                let retained_targets = obligation.retained_targets.clone();
                obligation
                    .results
                    .retain(|thread_id, _| !retained_targets.contains(thread_id));
                obligation.parent_turn_id = successor_turn_id.to_string();
                obligation.ready = false;
                obligation.trigger_sent = false;
                obligation.dispatch_complete = false;
                obligation.retained_delivery_predecessor_turn_id = None;
                obligation.parent_thread_id
            };
            if predecessor_turn_id != successor_turn_id
                && let Some(successor_turn_state) = successor_turn_state
            {
                store.continuations.push(JoinContinuationLease {
                    parent_thread_id,
                    generation,
                    predecessor_turn_id: predecessor_turn_id.to_string(),
                    successor_turn_id: successor_turn_id.to_string(),
                    successor_turn_state: Arc::clone(successor_turn_state),
                    terminal_status: None,
                });
            }
            return Some(true);
        }

        let obligation = store.obligations.remove(index);
        if !obligation.token.consume_if_current(generation) {
            return None;
        }
        let installs_continuation = obligation.trigger_turn
            && predecessor_turn_id != successor_turn_id
            && successor_turn_state.is_some();
        if let Some(successor_turn_state) = successor_turn_state
            && installs_continuation
        {
            store.continuations.push(JoinContinuationLease {
                parent_thread_id: obligation.parent_thread_id,
                generation,
                predecessor_turn_id: predecessor_turn_id.to_string(),
                successor_turn_id: successor_turn_id.to_string(),
                successor_turn_state: Arc::clone(successor_turn_state),
                terminal_status: None,
            });
        }
        Some(installs_continuation)
    }

    /// Acknowledge a ready aggregate exactly once. Retained generations transfer
    /// ownership to their delivery turn; ordinary generations are consumed.
    pub(crate) async fn consume_ready_join_obligation(
        &self,
        parent_turn_id: &str,
        generation: u64,
    ) -> bool {
        let mut store = self.join_obligations.lock().await;
        let Some(revision_changed) = Self::acknowledge_ready_join_delivery_locked(
            &mut store,
            parent_turn_id,
            generation,
            parent_turn_id,
            None,
        ) else {
            return false;
        };
        drop(store);
        if revision_changed {
            self.bump_join_continuation_revision();
        }
        true
    }

    async fn acknowledge_ready_join_obligation_for_turn(
        &self,
        predecessor_turn_id: &str,
        generation: u64,
        successor_turn_id: &str,
        successor_turn_state: &Arc<Mutex<TurnState>>,
    ) -> bool {
        let mut store = self.join_obligations.lock().await;
        let Some(revision_changed) = Self::acknowledge_ready_join_delivery_locked(
            &mut store,
            predecessor_turn_id,
            generation,
            successor_turn_id,
            Some(successor_turn_state),
        ) else {
            return false;
        };
        drop(store);
        if revision_changed {
            self.bump_join_continuation_revision();
        }
        true
    }

    pub(crate) async fn consume_ready_join_obligation_for_targets(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        parent_turn_id: &str,
        target_ids: &[codex_protocol::ThreadId],
        current_targets: &std::collections::HashMap<
            codex_protocol::ThreadId,
            (Arc<dyn Any + Send + Sync>, String),
        >,
    ) -> bool {
        self.consume_ready_join_obligation_for_targets_with_outcomes(
            parent_thread_id,
            parent_turn_id,
            target_ids,
            current_targets,
        )
        .await
        .is_some()
    }

    pub(crate) async fn consume_ready_join_obligation_for_targets_with_outcomes(
        &self,
        parent_thread_id: codex_protocol::ThreadId,
        parent_turn_id: &str,
        target_ids: &[codex_protocol::ThreadId],
        current_targets: &std::collections::HashMap<
            codex_protocol::ThreadId,
            (Arc<dyn Any + Send + Sync>, String),
        >,
    ) -> Option<Vec<(codex_protocol::ThreadId, AgentStatus)>> {
        // Preserve the mailbox -> join lock order shared with startup and active delivery.
        let mut pending_mails = self.mailbox_pending_mails.lock().await;
        let mut store = self.join_obligations.lock().await;
        let mut matching = store
            .obligations
            .iter()
            .enumerate()
            .filter(|(_, obligation)| {
                obligation.parent_thread_id == parent_thread_id
                    && obligation.parent_turn_id == parent_turn_id
                    && obligation.ready
                    && obligation.dispatch_complete
                    && obligation.targets.len() == target_ids.len()
                    && target_ids.iter().all(|thread_id| {
                        let Some((expected_incarnation, expected_turn_id)) =
                            obligation.targets.get(thread_id)
                        else {
                            return false;
                        };
                        let Some((current_incarnation, current_turn_id)) =
                            current_targets.get(thread_id)
                        else {
                            return false;
                        };
                        !expected_turn_id.is_empty()
                            && Arc::ptr_eq(expected_incarnation, current_incarnation)
                            && (expected_turn_id == current_turn_id
                                || (current_turn_id.is_empty()
                                    && obligation.results.contains_key(thread_id)))
                    })
            });
        let Some((index, _)) = matching.next() else {
            return None;
        };
        if matching.next().is_some() {
            // An unqualified targetful wait must not consume an older or
            // otherwise ambiguous parent-turn generation.
            return None;
        }
        let generation = store.obligations[index].token.generation();
        let mut outcomes = store.obligations[index]
            .results
            .iter()
            .map(|(thread_id, status)| (*thread_id, status.clone()))
            .collect::<Vec<_>>();
        outcomes.sort_by_key(|(thread_id, _)| thread_id.to_string());
        let revision_changed = Self::acknowledge_ready_join_delivery_locked(
            &mut store,
            parent_turn_id,
            generation,
            parent_turn_id,
            None,
        )?;
        drop(store);
        pending_mails.retain(|mail| {
            !matches!(
                mail.provenance,
                MailboxProvenance::JoinAggregate { generation: mail_generation }
                    if mail_generation == generation
            )
        });
        drop(pending_mails);
        if revision_changed {
            self.bump_join_continuation_revision();
        }
        Some(outcomes)
    }

    pub(crate) async fn finish_join_dispatch(&self, generation: u64) {
        let mut store = self.join_obligations.lock().await;
        if let Some(obligation) = store
            .obligations
            .iter_mut()
            .find(|obligation| obligation.token.generation() == generation && obligation.ready)
        {
            obligation.dispatch_complete = true;
        }
    }

    pub(crate) fn notify_wait_obligation_ready(&self) {
        self.activity_tx.send_replace(InputQueueActivity::Mailbox);
    }

    pub(in crate::session::input_queue) async fn has_ready_wait_obligation(&self) -> bool {
        self.join_obligations
            .lock()
            .await
            .obligations
            .iter()
            .any(|obligation| obligation.ready && !obligation.trigger_turn)
    }
}

pub(in crate::session::input_queue) fn mailbox_join_provenance(
    pending_mails: &[&PendingMailboxCommunication],
) -> Vec<(String, u64)> {
    pending_mails
        .iter()
        .filter_map(|mail| {
            if !mail.communication.trigger_turn {
                return None;
            }
            let MailboxProvenance::JoinAggregate { generation } = mail.provenance else {
                return None;
            };
            mail.start_options
                .parent_turn_id
                .as_deref()
                .filter(|parent_turn_id| !parent_turn_id.trim().is_empty())
                .map(|parent_turn_id| (parent_turn_id.to_string(), generation))
        })
        .collect()
}

pub(in crate::session::input_queue) fn validated_mailbox_join_provenance(
    pending_mails: &VecDeque<PendingMailboxCommunication>,
    store: &JoinObligationStore,
    successor_turn_id: Option<&str>,
) -> Option<Vec<(String, u64)>> {
    let mail = pending_mails.iter().collect::<Vec<_>>();
    let consumed_joins = mailbox_join_provenance(&mail);
    let tagged = pending_mails
        .iter()
        .filter(|mail| matches!(mail.provenance, MailboxProvenance::JoinAggregate { .. }))
        .count();
    if consumed_joins.len() != tagged {
        return None;
    }
    let mut generations = std::collections::HashSet::new();
    for (predecessor, generation) in &consumed_joins {
        if !generations.insert(*generation) {
            return None;
        }
        let mut matching = store.obligations.iter().filter(|obligation| {
            obligation.token.generation() == *generation
                && ((obligation.parent_turn_id == *predecessor && obligation.ready)
                    || obligation.retained_delivery_predecessor_turn_id.as_deref()
                        == Some(predecessor.as_str()))
        });
        let obligation = matching.next()?;
        let retained = obligation.retained_delivery_predecessor_turn_id.as_deref()
            == Some(predecessor.as_str());
        if matching.next().is_some()
            || !obligation.dispatch_complete
            || (!retained && obligation.token.consumed.load(Ordering::Acquire))
        {
            return None;
        }
        let successor = successor_turn_id.unwrap_or(predecessor);
        if obligation.trigger_turn
            && (successor.is_empty()
                || (successor != predecessor
                    && store.continuations.iter().any(|continuation| {
                        continuation.parent_thread_id == obligation.parent_thread_id
                            && continuation.predecessor_turn_id == *predecessor
                    })))
        {
            return None;
        }
    }
    Some(consumed_joins)
}
