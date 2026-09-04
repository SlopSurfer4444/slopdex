use super::AgentControl;
use crate::TurnStartOptions;
use codex_protocol::ThreadId;
use codex_protocol::protocol::InterAgentCommunication;
use std::sync::Arc;
use tokio::sync::watch;

pub(super) struct CapacityReadyLease {
    pub(super) parent_thread_id: ThreadId,
    pub(super) parent_turn_id: String,
    pub(super) generation: u64,
    pub(super) parent_thread: Arc<crate::codex_thread::CodexThread>,
    pub(super) dispatching: bool,
}

#[cfg(test)]
pub(super) struct CapacityReadyLeaseRetirementWatch {
    parent_thread_id: ThreadId,
    parent_turn_id: String,
    generation: u64,
    parent_thread: Arc<crate::codex_thread::CodexThread>,
    retired: Arc<tokio::sync::Notify>,
}

impl AgentControl {
    #[cfg(test)]
    pub(crate) async fn set_capacity_ready_barrier(
        &self,
        barrier: crate::tasks::PendingWakeClaimBarrier,
    ) {
        *self.capacity_ready_barrier.lock().await = Some(barrier);
    }

    #[cfg(test)]
    pub(crate) async fn watch_capacity_ready_lease_retirement(
        &self,
        parent_thread_id: ThreadId,
        parent_turn_id: String,
        generation: u64,
        parent_thread: Arc<crate::codex_thread::CodexThread>,
    ) -> Arc<tokio::sync::Notify> {
        let retired = Arc::new(tokio::sync::Notify::new());
        *self.capacity_ready_lease_retirement.lock().await =
            Some(CapacityReadyLeaseRetirementWatch {
                parent_thread_id,
                parent_turn_id,
                generation,
                parent_thread,
                retired: Arc::clone(&retired),
            });
        retired
    }
    pub(super) async fn retain_capacity_ready_lease(
        &self,
        parent_thread: Arc<crate::codex_thread::CodexThread>,
        parent_turn_id: String,
        generation: u64,
        communication: InterAgentCommunication,
        start_options: TurnStartOptions,
        mut release_rx: watch::Receiver<u64>,
        release_generation: u64,
    ) {
        let parent_thread_id = parent_thread.session.thread_id;
        {
            let mut leases = self.capacity_ready_leases.lock().await;
            if leases.iter().any(|lease| {
                lease.parent_thread_id == parent_thread_id
                    && lease.parent_turn_id == parent_turn_id
                    && lease.generation == generation
                    && Arc::ptr_eq(&lease.parent_thread, &parent_thread)
            }) {
                return;
            }
            leases.push(CapacityReadyLease {
                parent_thread_id,
                parent_turn_id: parent_turn_id.clone(),
                generation,
                parent_thread: Arc::clone(&parent_thread),
                dispatching: false,
            });
        }

        parent_thread
            .session
            .input_queue
            .enqueue_mailbox_communication(communication, start_options)
            .await;

        let control = self.clone();
        tokio::spawn(async move {
            let mut observed_release_generation = release_generation;
            loop {
                while *release_rx.borrow() == observed_release_generation {
                    if release_rx.changed().await.is_err() {
                        control
                            .remove_capacity_ready_lease(
                                parent_thread_id,
                                &parent_turn_id,
                                generation,
                                &parent_thread,
                            )
                            .await;
                        return;
                    }
                }
                observed_release_generation = *release_rx.borrow();

                let should_dispatch = {
                    let mut leases = control.capacity_ready_leases.lock().await;
                    let Some(lease) = leases.iter_mut().find(|lease| {
                        lease.parent_thread_id == parent_thread_id
                            && lease.parent_turn_id == parent_turn_id
                            && lease.generation == generation
                            && Arc::ptr_eq(&lease.parent_thread, &parent_thread)
                    }) else {
                        return;
                    };
                    if lease.dispatching {
                        false
                    } else {
                        lease.dispatching = true;
                        true
                    }
                };
                if !should_dispatch {
                    continue;
                }

                let Ok(state) = control.upgrade() else {
                    control
                        .remove_capacity_ready_lease(
                            parent_thread_id,
                            &parent_turn_id,
                            generation,
                            &parent_thread,
                        )
                        .await;
                    return;
                };
                let Some(current_parent) = state.get_thread(parent_thread_id).await.ok() else {
                    control
                        .remove_capacity_ready_lease(
                            parent_thread_id,
                            &parent_turn_id,
                            generation,
                            &parent_thread,
                        )
                        .await;
                    return;
                };
                if !Arc::ptr_eq(&current_parent, &parent_thread) {
                    control
                        .remove_capacity_ready_lease(
                            parent_thread_id,
                            &parent_turn_id,
                            generation,
                            &parent_thread,
                        )
                        .await;
                    return;
                }

                // The native pending-work admission arbitrates against an active/user turn.
                // If it loses that race, the trigger mail remains queued for the next normal
                // completion path; this watcher re-arms for a later release.
                #[cfg(test)]
                let barrier = control.capacity_ready_barrier.lock().await.take();
                #[cfg(test)]
                if let Some(barrier) = barrier {
                    parent_thread
                        .session
                        .maybe_start_turn_for_pending_work_with_sub_id_and_barrier(
                            format!("capacity-ready-{generation}"),
                            barrier,
                        )
                        .await;
                } else {
                    parent_thread
                        .session
                        .maybe_start_turn_for_pending_work()
                        .await;
                }
                #[cfg(not(test))]
                parent_thread
                    .session
                    .maybe_start_turn_for_pending_work()
                    .await;
                if parent_thread
                    .session
                    .input_queue
                    .has_pending_join_generation(generation)
                    .await
                {
                    let mut leases = control.capacity_ready_leases.lock().await;
                    if let Some(lease) = leases.iter_mut().find(|lease| {
                        lease.parent_thread_id == parent_thread_id
                            && lease.parent_turn_id == parent_turn_id
                            && lease.generation == generation
                            && Arc::ptr_eq(&lease.parent_thread, &parent_thread)
                    }) {
                        lease.dispatching = false;
                    }
                    continue;
                }
                control
                    .remove_capacity_ready_lease(
                        parent_thread_id,
                        &parent_turn_id,
                        generation,
                        &parent_thread,
                    )
                    .await;
                return;
            }
        });
    }

    async fn remove_capacity_ready_lease(
        &self,
        parent_thread_id: ThreadId,
        parent_turn_id: &str,
        generation: u64,
        parent_thread: &Arc<crate::codex_thread::CodexThread>,
    ) {
        #[cfg(test)]
        let removed = {
            let mut leases = self.capacity_ready_leases.lock().await;
            let previous_len = leases.len();
            leases.retain(|lease| {
                !(lease.parent_thread_id == parent_thread_id
                    && lease.parent_turn_id == parent_turn_id
                    && lease.generation == generation
                    && Arc::ptr_eq(&lease.parent_thread, parent_thread))
            });
            leases.len() != previous_len
        };
        #[cfg(not(test))]
        self.capacity_ready_leases.lock().await.retain(|lease| {
            !(lease.parent_thread_id == parent_thread_id
                && lease.parent_turn_id == parent_turn_id
                && lease.generation == generation
                && Arc::ptr_eq(&lease.parent_thread, parent_thread))
        });
        #[cfg(test)]
        if removed {
            let retired = {
                let mut watch = self.capacity_ready_lease_retirement.lock().await;
                let matches = watch.as_ref().is_some_and(|watch| {
                    watch.parent_thread_id == parent_thread_id
                        && watch.parent_turn_id == parent_turn_id
                        && watch.generation == generation
                        && Arc::ptr_eq(&watch.parent_thread, parent_thread)
                });
                matches.then(|| {
                    watch
                        .take()
                        .expect("retirement watch should remain installed")
                        .retired
                })
            };
            if let Some(retired) = retired {
                retired.notify_one();
            }
        }
    }
}
