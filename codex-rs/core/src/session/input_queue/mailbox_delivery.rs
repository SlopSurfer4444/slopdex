use super::InputQueue;
use super::InputQueueActivity;
use super::MailboxProvenance;
use super::PENDING_MAILBOX_MESSAGES;
use super::PendingMailboxCommunication;
use super::TurnInput;
use super::owned_join::mailbox_join_provenance;
use super::owned_join::validated_mailbox_join_provenance;
use crate::state::ActiveTurn;
use crate::state::TurnState;
use crate::tasks::TasklessTurnClaim;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::turn_input::TurnStartOptions;
use std::sync::Arc;
use tokio::sync::Mutex;

impl InputQueue {
    pub(crate) async fn enqueue_mailbox_communication(
        &self,
        communication: InterAgentCommunication,
        start_options: TurnStartOptions,
    ) {
        let provenance = if communication.trigger_turn {
            let parent_turn_id = start_options.parent_turn_id.as_deref();
            let mut pending = self.join_trigger_provenance.lock().await;
            parent_turn_id
                .and_then(|parent_turn_id| pending.remove(parent_turn_id))
                .map_or(MailboxProvenance::Ordinary, |generation| {
                    MailboxProvenance::JoinAggregate { generation }
                })
        } else {
            MailboxProvenance::Ordinary
        };
        let dispatched_generation = match provenance {
            MailboxProvenance::JoinAggregate { generation } => Some(generation),
            MailboxProvenance::Ordinary => None,
        };
        self.mailbox_pending_mails
            .lock()
            .await
            .push_back(PendingMailboxCommunication {
                communication,
                start_options,
                provenance,
                _diagnostics_guard: PENDING_MAILBOX_MESSAGES.track(),
            });
        if let Some(generation) = dispatched_generation {
            self.finish_join_dispatch(generation).await;
        }
        self.activity_tx.send_replace(InputQueueActivity::Mailbox);
    }

    pub(crate) async fn mark_join_trigger(&self, parent_turn_id: &str, generation: u64) {
        self.join_trigger_provenance
            .lock()
            .await
            .insert(parent_turn_id.to_string(), generation);
    }

    pub(crate) async fn clear_join_trigger(&self, parent_turn_id: &str, generation: u64) {
        let mut pending = self.join_trigger_provenance.lock().await;
        if pending.get(parent_turn_id) == Some(&generation) {
            pending.remove(parent_turn_id);
        }
    }

    pub(crate) async fn has_pending_mailbox_items(&self) -> bool {
        !self.mailbox_pending_mails.lock().await.is_empty()
    }

    pub(crate) async fn has_trigger_turn_mailbox_items(&self) -> bool {
        self.mailbox_pending_mails
            .lock()
            .await
            .iter()
            .any(|mail| mail.communication.trigger_turn)
    }

    /// Reports whether this exact join generation still owns either its trigger mail or
    /// retained aggregate. Capacity wake leases use this query to distinguish a rejected
    /// native admission from exact downstream consumption without inspecting unrelated mail.
    pub(crate) async fn has_pending_join_generation(&self, generation: u64) -> bool {
        if self.mailbox_pending_mails.lock().await.iter().any(|mail| {
            matches!(
                mail.provenance,
                MailboxProvenance::JoinAggregate {
                    generation: mail_generation
                } if mail_generation == generation
            )
        }) {
            return true;
        }
        self.join_obligations
            .lock()
            .await
            .obligations
            .iter()
            .any(|obligation| obligation.token.generation() == generation)
    }

    #[cfg(test)]
    pub(crate) async fn drain_mailbox_input_items(&self) -> (Vec<TurnInput>, TurnStartOptions) {
        let (items, start_options, _) = self.drain_mailbox_input_items_with_provenance().await;
        (items, start_options)
    }

    pub(crate) async fn preview_mailbox_input_items(&self) -> (Vec<TurnInput>, TurnStartOptions) {
        let pending_mails = self.mailbox_pending_mails.lock().await;
        let pending_mail_refs = pending_mails.iter().collect::<Vec<_>>();
        let start_options = mailbox_start_options(&pending_mail_refs);
        let items = pending_mails
            .iter()
            .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone()))
            .collect();
        (items, start_options)
    }

    /// Atomically commits the mailbox batch into the exact taskless turn that will run it.
    /// The caller holds the session's active-turn guard across this method and installs the
    /// task synchronously after it returns.
    pub(crate) async fn commit_startup_mailbox_for_turn(
        &self,
        active_turn: &mut ActiveTurn,
        expected_claim: &Arc<TasklessTurnClaim>,
        expected_turn_state: &Arc<Mutex<TurnState>>,
        successor_turn_id: &str,
        token_usage_at_turn_start: TokenUsage,
    ) -> Option<TurnStartOptions> {
        if !matches_taskless_turn(active_turn, expected_claim, expected_turn_state) {
            return None;
        }

        // Keep the established mailbox -> join ordering. Cancellation while acquiring any lock
        // below releases the already-acquired guards before the first shared-state mutation.
        let mut pending_mails = self.mailbox_pending_mails.lock().await;
        let mut join_obligations = self.join_obligations.lock().await;
        let mut turn_state = expected_turn_state.lock().await;
        if !matches_taskless_turn(active_turn, expected_claim, expected_turn_state) {
            return None;
        }

        let (start_options, consumed_joins) = {
            let pending_mail_refs = pending_mails.iter().collect::<Vec<_>>();
            (
                mailbox_start_options(&pending_mail_refs),
                mailbox_join_provenance(&pending_mail_refs),
            )
        };
        for (predecessor_turn_id, generation) in &consumed_joins {
            let Some(obligation) = join_obligations.obligations.iter().find(|obligation| {
                obligation.token.generation() == *generation
                    && ((obligation.parent_turn_id == *predecessor_turn_id && obligation.ready)
                        || obligation.retained_delivery_predecessor_turn_id.as_deref()
                            == Some(predecessor_turn_id.as_str()))
            }) else {
                continue;
            };
            if obligation.trigger_turn
                && (successor_turn_id.is_empty()
                    || predecessor_turn_id.is_empty()
                    || predecessor_turn_id == successor_turn_id
                    || join_obligations.continuations.iter().any(|continuation| {
                        continuation.parent_thread_id == obligation.parent_thread_id
                            && continuation.predecessor_turn_id == *predecessor_turn_id
                    }))
            {
                return None;
            }
        }
        let items = pending_mails
            .drain(..)
            .map(|mail| TurnInput::InterAgentCommunication(mail.communication));
        turn_state.token_usage_at_turn_start = token_usage_at_turn_start;
        turn_state.pending_input.items.extend(items);
        for (parent_turn_id, generation) in consumed_joins {
            if Self::acknowledge_ready_join_delivery_locked(
                &mut join_obligations,
                &parent_turn_id,
                generation,
                successor_turn_id,
                Some(expected_turn_state),
            ) == Some(true)
            {
                self.bump_join_continuation_revision();
            }
        }
        Some(start_options)
    }

    async fn drain_mailbox_input_items_with_provenance(
        &self,
    ) -> (Vec<TurnInput>, TurnStartOptions, Vec<(String, u64)>) {
        let pending_mails = self
            .mailbox_pending_mails
            .lock()
            .await
            .drain(..)
            .collect::<Vec<_>>();
        let (start_options, consumed_joins) = {
            let pending_mail_refs = pending_mails.iter().collect::<Vec<_>>();
            (
                mailbox_start_options(&pending_mail_refs),
                mailbox_join_provenance(&pending_mail_refs),
            )
        };
        let items = pending_mails
            .into_iter()
            .map(|mail| TurnInput::InterAgentCommunication(mail.communication))
            .collect();
        (items, start_options, consumed_joins)
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "active turn checks and turn state updates must remain atomic"
    )]
    pub(crate) async fn get_pending_input(
        &self,
        active_turn: &Mutex<Option<ActiveTurn>>,
    ) -> (Vec<TurnInput>, TurnStartOptions) {
        let (active_turn_id, active_turn_state, active_turn_metadata) = {
            let mut active = active_turn.lock().await;
            match active.as_mut() {
                Some(active_turn) => {
                    let active_turn_metadata = active_turn
                        .task
                        .as_ref()
                        .map(|task| Arc::clone(&task.turn_context.turn_metadata_state));
                    (
                        active_turn
                            .task
                            .as_ref()
                            .map(|task| task.turn_context.sub_id.clone()),
                        Some(Arc::clone(&active_turn.turn_state)),
                        active_turn_metadata,
                    )
                }
                None => (None, None, None),
            }
        };

        {
            let active = active_turn.lock().await;
            if !matches_active_delivery(
                &active,
                active_turn_id.as_deref(),
                active_turn_state.as_ref(),
            ) {
                return (Vec::new(), TurnStartOptions::default());
            }
            let pending_mails = self.mailbox_pending_mails.lock().await;
            let join_obligations = self.join_obligations.lock().await;
            let state = match active_turn_state.as_ref() {
                Some(turn_state) => Some(turn_state.lock().await),
                None => None,
            };
            if state
                .as_ref()
                .is_some_and(|turn_state| !turn_state.accepts_mailbox_delivery_for_current_turn())
                || validated_mailbox_join_provenance(
                    &pending_mails,
                    &join_obligations,
                    active_turn_id.as_deref(),
                )
                .is_none()
            {
                return (Vec::new(), TurnStartOptions::default());
            }
        }

        #[cfg(test)]
        {
            let barrier = self.active_delivery_ack_barrier.lock().await.take();
            if let Some((reached, release)) = barrier {
                reached.notify_one();
                release.notified().await;
            }
        }

        let active = active_turn.lock().await;
        if !matches_active_delivery(
            &active,
            active_turn_id.as_deref(),
            active_turn_state.as_ref(),
        ) {
            return (Vec::new(), TurnStartOptions::default());
        }
        let mut pending_mails = self.mailbox_pending_mails.lock().await;
        let mut join_obligations = self.join_obligations.lock().await;
        let mut state = match active_turn_state.as_ref() {
            Some(turn_state) => Some(turn_state.lock().await),
            None => None,
        };
        if state
            .as_ref()
            .is_some_and(|turn_state| !turn_state.accepts_mailbox_delivery_for_current_turn())
        {
            return (Vec::new(), TurnStartOptions::default());
        }
        let pending_mail_refs = pending_mails.iter().collect::<Vec<_>>();
        let start_options = mailbox_start_options(&pending_mail_refs);
        let Some(consumed_joins) = validated_mailbox_join_provenance(
            &pending_mails,
            &join_obligations,
            active_turn_id.as_deref(),
        ) else {
            return (Vec::new(), TurnStartOptions::default());
        };
        let mut pending_input = state
            .as_mut()
            .map(|turn_state| turn_state.pending_input.items.split_off(0))
            .unwrap_or_default();
        pending_input.extend(
            pending_mails
                .drain(..)
                .map(|mail| TurnInput::InterAgentCommunication(mail.communication)),
        );
        let mut revision_changed = false;
        for (parent_turn_id, generation) in consumed_joins {
            revision_changed |= Self::acknowledge_ready_join_delivery_locked(
                &mut join_obligations,
                &parent_turn_id,
                generation,
                active_turn_id.as_deref().unwrap_or(&parent_turn_id),
                active_turn_state.as_ref(),
            )
            .expect("validated mailbox generation must remain acknowledgeable");
        }
        if let Some(active_turn_metadata) = active_turn_metadata
            && active_turn_metadata.root_turn_id().is_none()
            && let Some(root_turn_id) = start_options.root_turn_id.as_ref()
        {
            active_turn_metadata.set_root_turn_id(root_turn_id.clone());
        }
        if revision_changed {
            self.bump_join_continuation_revision();
        }
        (pending_input, start_options)
    }
}

fn matches_taskless_turn(
    active_turn: &ActiveTurn,
    expected_claim: &Arc<TasklessTurnClaim>,
    expected_turn_state: &Arc<Mutex<TurnState>>,
) -> bool {
    active_turn.task.is_none()
        && Arc::ptr_eq(&active_turn.turn_state, expected_turn_state)
        && active_turn
            .taskless_start_claim()
            .is_some_and(|claim| Arc::ptr_eq(&claim, expected_claim))
}

fn mailbox_start_options(pending_mails: &[&PendingMailboxCommunication]) -> TurnStartOptions {
    // A later follow-up supersedes the earlier choice, including an omitted choice.
    let mut start_options = pending_mails
        .iter()
        .rev()
        .find(|mail| mail.communication.trigger_turn)
        .map(|mail| mail.start_options.clone())
        .unwrap_or_default();
    start_options.parent_turn_id = pending_mails
        .iter()
        .filter(|mail| mail.communication.trigger_turn)
        .map(|mail| mail.start_options.parent_turn_id.as_deref())
        .reduce(|expected, candidate| expected.filter(|id| candidate == Some(*id)))
        .and_then(|id| id.filter(|id| !id.trim().is_empty()).map(str::to_string));
    start_options.root_turn_id = pending_mails
        .iter()
        .find(|mail| mail.communication.trigger_turn)
        .and_then(|mail| {
            mail.start_options
                .parent_turn_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .and(mail.start_options.root_turn_id.as_deref())
                .filter(|id| !id.trim().is_empty())
        })
        .map(str::to_string);
    start_options
}

fn matches_active_delivery(
    active: &Option<ActiveTurn>,
    expected_id: Option<&str>,
    expected_state: Option<&Arc<Mutex<TurnState>>>,
) -> bool {
    match active.as_ref() {
        None => expected_state.is_none() && expected_id.is_none(),
        Some(active) => {
            expected_state.is_some_and(|state| Arc::ptr_eq(&active.turn_state, state))
                && active
                    .task
                    .as_ref()
                    .map(|task| task.turn_context.sub_id.as_str())
                    == expected_id
        }
    }
}
