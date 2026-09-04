use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn b6_r2_active_user_turn_retains_generation_for_fast_successor() {
    let harness = AgentControlHarness::new().await;
    let (parent_id, parent) = harness.start_thread().await;
    let barrier = crate::session::session::Session::new_user_start_claim_barrier();
    let parent_for_start = Arc::clone(&parent);
    let barrier_for_start = barrier.clone();
    let start = tokio::spawn(async move {
        parent_for_start
            .session
            .start_or_steer_with_task_and_barrier(
                TurnInputRequest::user_input(text_input("user priority")),
                "b6-user".into(),
                NeverEndingCapacityTask,
                barrier_for_start,
            )
            .await
    });
    barrier.wait_until_claimed().await;
    barrier.release();
    assert_matches!(
        start.await.expect("user join").expect("user start"),
        TurnInputSubmission::Started { .. }
    );
    let child_id = ThreadId::new();
    let child: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let generation = b6_seed_join(&parent, "A0", child_id, &child, "B1", true).await;
    b6_enqueue_join(&parent, "A0", generation).await;
    let mut revisions = parent.session.input_queue.subscribe_join_continuations();
    let revision = *revisions.borrow_and_update();
    assert_eq!(
        parent
            .session
            .input_queue
            .get_pending_input(&parent.session.active_turn)
            .await
            .0
            .len(),
        1
    );
    assert_eq!(
        parent
            .session
            .input_queue
            .join_owner_turn_for_generation(parent_id, generation)
            .await,
        Ok(Some("b6-user".into()))
    );
    assert!(*revisions.borrow_and_update() > revision);
    assert!(
        parent
            .session
            .input_queue
            .rebind_join_target_to_successor(parent_id, generation, child_id, &child, "B1", "B2",)
            .await
    );
    assert!(
        parent
            .session
            .input_queue
            .resolve_join_target(
                parent_id,
                generation,
                child_id,
                &child,
                "B2",
                AgentStatus::Completed(Some("B2".into())),
            )
            .await
            .is_some()
    );
    parent
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b6_r2_forced_active_delivery_cancellation_does_not_strand_retained_generation() {
    struct FetchPendingInputTask {
        begin: Arc<tokio::sync::Notify>,
        result: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<Vec<TurnInput>>>>,
    }

    impl SessionTask for FetchPendingInputTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "agent_control_tests.fetch_pending_input"
        }

        async fn run(
            self: Arc<Self>,
            session: Arc<crate::session::session::Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            self.begin.notified().await;
            let input = session
                .input_queue
                .get_pending_input(&session.active_turn)
                .await
                .0;
            if let Some(result) = self.result.lock().await.take() {
                let _ = result.send(input);
            }
            cancellation_token.cancelled().await;
            Ok(None)
        }
    }

    #[derive(Debug, PartialEq)]
    struct Observation {
        gap_reached: bool,
        mailbox_present_after_cancel: bool,
        generation_present_after_cancel: bool,
        owner_after_cancel: Option<String>,
        revision_delta_after_cancel: u64,
        retained_result_consumed_after_cancel: bool,
        first_successor_delivery_count: usize,
        first_successor_received_exact_aggregate: bool,
        owner_after_first_successor: Option<String>,
        revision_delta_after_first_successor: u64,
        retained_result_consumed_after_first_successor: bool,
        exact_rebind_succeeded: bool,
        second_successor_delivery_count: usize,
    }

    let harness = AgentControlHarness::new().await;
    let (parent_id, parent) = harness.start_thread().await;
    harness.control.register_session_root(parent_id, None);

    let a0_begin = Arc::new(tokio::sync::Notify::new());
    let (a0_result_tx, _a0_result_rx) = tokio::sync::oneshot::channel();
    let a0 = parent
        .session
        .new_turn_with_default_settings("A0".into(), Default::default())
        .await;
    parent
        .session
        .start_task(
            a0,
            Vec::new(),
            FetchPendingInputTask {
                begin: Arc::clone(&a0_begin),
                result: tokio::sync::Mutex::new(Some(a0_result_tx)),
            },
        )
        .await;

    let child_id = ThreadId::new();
    let child: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let generation = b6_seed_join(&parent, "A0", child_id, &child, "B1", true).await;
    b6_enqueue_join(&parent, "A0", generation).await;
    let mut revisions = parent.session.input_queue.subscribe_join_continuations();
    let initial_revision = *revisions.borrow_and_update();
    let preview = parent
        .session
        .input_queue
        .preview_mailbox_input_items()
        .await
        .0;
    assert_matches!(
        preview.as_slice(),
        [TurnInput::InterAgentCommunication(communication)]
            if communication.trigger_turn && communication.content == "join A0"
    );
    assert!(
        parent
            .session
            .input_queue
            .has_pending_join_generation(generation)
            .await
    );
    assert_eq!(
        parent
            .session
            .input_queue
            .join_owner_turn_for_generation(parent_id, generation)
            .await,
        Ok(Some("A0".into()))
    );
    assert_eq!(
        parent
            .session
            .input_queue
            .retained_join_delivery_state(parent_id, generation, child_id)
            .await
            .expect("generation lookup")
            .expect("retained delivery")
            .consumed_for_target,
        false
    );

    let gap_reached = Arc::new(tokio::sync::Notify::new());
    let hold_gap = Arc::new(tokio::sync::Notify::new());
    parent
        .session
        .input_queue
        .set_active_delivery_ack_barrier(Arc::clone(&gap_reached), Arc::clone(&hold_gap))
        .await;
    a0_begin.notify_one();
    timeout(Duration::from_secs(2), gap_reached.notified())
        .await
        .expect("real post-drain/pre-ACK gap should be reached");
    timeout(
        Duration::from_secs(5),
        parent
            .session
            .abort_all_tasks(TurnAbortReason::BudgetLimited),
    )
    .await
    .expect("forced abort should cancel the paused active delivery");

    let revision_after_cancel = *revisions.borrow_and_update();
    let mailbox_present_after_cancel = parent.session.input_queue.has_pending_mailbox_items().await;
    let generation_present_after_cancel = parent
        .session
        .input_queue
        .has_pending_join_generation(generation)
        .await;
    let owner_after_cancel = parent
        .session
        .input_queue
        .join_owner_turn_for_generation(parent_id, generation)
        .await
        .expect("generation owner lookup");
    let retained_result_consumed_after_cancel = parent
        .session
        .input_queue
        .retained_join_delivery_state(parent_id, generation, child_id)
        .await
        .expect("generation lookup")
        .expect("retained delivery")
        .consumed_for_target;

    let a1_begin = Arc::new(tokio::sync::Notify::new());
    let (a1_result_tx, a1_result_rx) = tokio::sync::oneshot::channel();
    let a1 = parent
        .session
        .new_turn_with_default_settings("A1".into(), Default::default())
        .await;
    parent
        .session
        .start_task(
            a1,
            Vec::new(),
            FetchPendingInputTask {
                begin: Arc::clone(&a1_begin),
                result: tokio::sync::Mutex::new(Some(a1_result_tx)),
            },
        )
        .await;
    a1_begin.notify_one();
    let first_successor_delivery = timeout(Duration::from_secs(2), a1_result_rx)
        .await
        .expect("first successor should query pending input")
        .expect("first successor should report pending input");
    let revision_after_first_successor = *revisions.borrow_and_update();
    let owner_after_first_successor = parent
        .session
        .input_queue
        .join_owner_turn_for_generation(parent_id, generation)
        .await
        .expect("generation owner lookup");
    let retained_result_consumed_after_first_successor = parent
        .session
        .input_queue
        .retained_join_delivery_state(parent_id, generation, child_id)
        .await
        .expect("generation lookup")
        .expect("retained delivery")
        .consumed_for_target;
    let exact_rebind_succeeded = parent
        .session
        .input_queue
        .rebind_join_target_to_successor(parent_id, generation, child_id, &child, "B1", "B2")
        .await;
    parent
        .session
        .abort_all_tasks(TurnAbortReason::BudgetLimited)
        .await;

    let a2_begin = Arc::new(tokio::sync::Notify::new());
    let (a2_result_tx, a2_result_rx) = tokio::sync::oneshot::channel();
    let a2 = parent
        .session
        .new_turn_with_default_settings("A2".into(), Default::default())
        .await;
    parent
        .session
        .start_task(
            a2,
            Vec::new(),
            FetchPendingInputTask {
                begin: Arc::clone(&a2_begin),
                result: tokio::sync::Mutex::new(Some(a2_result_tx)),
            },
        )
        .await;
    a2_begin.notify_one();
    let second_successor_delivery = timeout(Duration::from_secs(2), a2_result_rx)
        .await
        .expect("second successor should query pending input")
        .expect("second successor should report pending input");
    parent
        .session
        .abort_all_tasks(TurnAbortReason::BudgetLimited)
        .await;

    let first_successor_received_exact_aggregate = matches!(
        first_successor_delivery.as_slice(),
        [TurnInput::InterAgentCommunication(communication)]
            if communication.trigger_turn && communication.content == "join A0"
    );
    assert_eq!(
        Observation {
            gap_reached: true,
            mailbox_present_after_cancel,
            generation_present_after_cancel,
            owner_after_cancel,
            revision_delta_after_cancel: revision_after_cancel - initial_revision,
            retained_result_consumed_after_cancel,
            first_successor_delivery_count: first_successor_delivery.len(),
            first_successor_received_exact_aggregate,
            owner_after_first_successor,
            revision_delta_after_first_successor: revision_after_first_successor - initial_revision,
            retained_result_consumed_after_first_successor,
            exact_rebind_succeeded,
            second_successor_delivery_count: second_successor_delivery.len(),
        },
        Observation {
            gap_reached: true,
            mailbox_present_after_cancel: true,
            generation_present_after_cancel: true,
            owner_after_cancel: Some("A0".into()),
            revision_delta_after_cancel: 0,
            retained_result_consumed_after_cancel: false,
            first_successor_delivery_count: 1,
            first_successor_received_exact_aggregate: true,
            owner_after_first_successor: Some("A1".into()),
            revision_delta_after_first_successor: 1,
            retained_result_consumed_after_first_successor: true,
            exact_rebind_succeeded: true,
            second_successor_delivery_count: 0,
        }
    );
}

#[tokio::test]
async fn b6_r2_completed_root_releases_join_continuation_lease() {
    b6_assert_root_releases_exact_join_continuation(B6RootTermination::Completed).await;
}

#[tokio::test]
async fn b6_r2_interrupted_root_releases_join_continuation_lease() {
    b6_assert_root_releases_exact_join_continuation(B6RootTermination::Aborted(
        TurnAbortReason::Interrupted,
    ))
    .await;
}

#[tokio::test]
async fn b6_r2_budget_limited_root_releases_join_continuation_lease() {
    b6_assert_root_releases_exact_join_continuation(B6RootTermination::Aborted(
        TurnAbortReason::BudgetLimited,
    ))
    .await;
}

#[tokio::test]
async fn b6_r2_observer_rejects_later_turn_terminal_but_accepts_shutdown() {
    for later in [
        AgentStatus::Completed(Some("B3".into())),
        AgentStatus::Errored("B3".into()),
    ] {
        let harness = AgentControlHarness::new().await;
        let (parent_id, parent) = harness.start_thread().await;
        harness.control.register_session_root(parent_id, None);
        let turn = parent
            .session
            .new_turn_with_default_settings("A0".into(), Default::default())
            .await;
        parent
            .session
            .start_task(turn, Vec::new(), NeverEndingCapacityTask)
            .await;
        let (child_id, child) =
            b6_spawn_pathful_child(&harness, parent_id, &parent, "A0", "child").await;
        child
            .session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await;
        let _ = b6_gated_turn(&child, "B2").await;
        assert!(
            harness
                .control
                .register_join_obligation(parent_id, "A0".into(), vec![child_id],)
                .await
                .expect("register")
        );
        child
            .session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await;
        b6_wait_status(&child, AgentStatus::Interrupted).await;
        let (b3, b3_state, _) = b6_gated_turn(&child, "B3").await;
        child
            .session
            .input_queue
            .record_nested_join_turn_terminal(child_id, "B3", &b3_state, later.clone())
            .await;
        let error = matches!(later, AgentStatus::Errored(_)).then(|| ErrorEvent {
            message: "B3".into(),
            codex_error_info: None,
            misalignment: None,
        });
        child
            .session
            .send_event(
                b3.as_ref(),
                EventMsg::TurnComplete(TurnCompleteEvent {
                    turn_id: "B3".into(),
                    started_at: None,
                    last_agent_message: None,
                    error,
                    completed_at: None,
                    duration_ms: None,
                    time_to_first_token_ms: None,
                }),
            )
            .await;
        sleep(Duration::from_millis(100)).await;
        let (pending_input, _) = parent
            .session
            .input_queue
            .preview_mailbox_input_items()
            .await;
        assert!(!pending_input.iter().any(|input| {
            matches!(
                input,
                TurnInput::InterAgentCommunication(communication)
                    if communication.trigger_turn
            )
        }));
        parent
            .session
            .input_queue
            .get_pending_input(&parent.session.active_turn)
            .await;
        assert!(
            !parent
                .session
                .input_queue
                .has_pending_input(&parent.session.active_turn)
                .await
        );
        child
            .session
            .send_event(b3.as_ref(), EventMsg::ShutdownComplete)
            .await;
        timeout(Duration::from_secs(2), async {
            while !parent
                .session
                .input_queue
                .has_pending_input(&parent.session.active_turn)
                .await
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Shutdown should discharge exact B2");
        assert_eq!(
            parent
                .session
                .input_queue
                .get_pending_input(&parent.session.active_turn)
                .await
                .0
                .len(),
            1
        );
        parent
            .session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await;
        child
            .session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await;
    }
}

#[tokio::test]
async fn b6_r2_observer_subscribes_before_target_snapshot() {
    let harness = AgentControlHarness::new().await;
    let (parent_id, parent) = harness.start_thread().await;
    let parent_turn = parent
        .session
        .new_turn_with_default_settings("A0".into(), Default::default())
        .await;
    parent
        .session
        .start_task(parent_turn, Vec::new(), NeverEndingCapacityTask)
        .await;
    let (b1_id, b1) = b6_spawn_pathful_child(&harness, parent_id, &parent, "A0", "b1").await;
    let (c1_id, c1) = b6_spawn_pathful_child(&harness, parent_id, &parent, "A0", "c1").await;
    assert_ne!(
        harness
            .control
            .get_agent_metadata(b1_id)
            .and_then(|metadata| metadata.agent_path),
        harness
            .control
            .get_agent_metadata(c1_id)
            .and_then(|metadata| metadata.agent_path)
    );
    b1.session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    c1.session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    let (_, b1_state, release_b1) = b6_gated_turn(&b1, "B1").await;
    let (_, _, release_c1) = b6_gated_turn(&c1, "C1").await;
    let c1_guard = c1.session.active_turn.lock().await;
    let baseline = Arc::strong_count(&b1_state);
    let control = harness.control.clone();
    let register = tokio::spawn(async move {
        control
            .register_join_obligation(parent_id, "A0".into(), vec![b1_id, c1_id])
            .await
    });
    timeout(Duration::from_secs(2), async {
        while Arc::strong_count(&b1_state) == baseline {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("registration should snapshot B1 before blocking on C1");
    release_b1.notify_one();
    b6_wait_status(
        &b1,
        AgentStatus::Completed(Some("completed after release".into())),
    )
    .await;
    assert!(b1.session.active_turn.lock().await.is_none());
    drop(c1_guard);
    assert!(
        register
            .await
            .expect("registration task")
            .expect("registration")
    );
    assert!(
        parent
            .session
            .input_queue
            .has_pending_join_generation(1)
            .await
    );
    assert_eq!(
        parent
            .session
            .input_queue
            .join_owner_turn_for_generation(parent_id, 1)
            .await,
        Ok(Some("A0".into()))
    );
    assert_eq!(
        parent
            .session
            .input_queue
            .join_owner_turn_for_generation(parent_id, 2)
            .await,
        Ok(None)
    );
    release_c1.notify_one();
    b6_wait_status(
        &c1,
        AgentStatus::Completed(Some("completed after release".into())),
    )
    .await;
    assert!(c1.session.active_turn.lock().await.is_none());
    timeout(Duration::from_secs(2), async {
        loop {
            let (input, _) = parent
                .session
                .input_queue
                .preview_mailbox_input_items()
                .await;
            if input.iter().any(|input| {
                matches!(
                    input,
                    TurnInput::InterAgentCommunication(communication)
                        if communication.trigger_turn
                            && communication.content.contains(&b1_id.to_string())
                            && communication.content.contains(&c1_id.to_string())
                )
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both exact child terminals should produce one parent aggregate");
}

#[tokio::test]
async fn b6_r2_targetful_ack_wakes_observer_without_owner_change() {
    let fixture = b5_capacity_fixture().await;
    let harness = &fixture.harness;
    fixture
        .target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    let parent_turn = fixture
        .parent_thread
        .session
        .new_turn_with_default_settings("A0".into(), Default::default())
        .await;
    fixture
        .parent_thread
        .session
        .start_task(parent_turn, Vec::new(), NeverEndingCapacityTask)
        .await;
    let (_, _, release_b1) = b6_gated_turn(&fixture.target_thread, "B1").await;
    let dummy_id = ThreadId::new();
    let dummy: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let inner_generation = b6_seed_join(
        &fixture.target_thread,
        "B1",
        dummy_id,
        &dummy,
        "leaf",
        false,
    )
    .await;
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                "A0".into(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("outer A0 to B1 registration")
    );
    release_b1.notify_one();
    timeout(Duration::from_secs(2), async {
        while fixture
            .target_thread
            .session
            .active_turn
            .lock()
            .await
            .is_some()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("B1 should complete and become idle");
    timeout(Duration::from_secs(2), async {
        while !fixture
            .parent_thread
            .session
            .input_queue
            .has_pending_mailbox_items()
            .await
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("B1 outer aggregate should reach the active parent");
    let mut parent_revisions = fixture
        .parent_thread
        .session
        .input_queue
        .subscribe_join_continuations();
    let revision = *parent_revisions.borrow_and_update();
    assert_eq!(
        fixture
            .parent_thread
            .session
            .input_queue
            .join_owner_turn_for_generation(fixture.parent_thread_id, 1)
            .await,
        Ok(Some("A0".into()))
    );
    assert_eq!(
        harness
            .control
            .consume_ready_join_obligation_for_targets_with_outcomes(
                fixture.parent_thread_id,
                "A0",
                &[fixture.target_thread_id],
            )
            .await
            .expect("targetful wait should reach the parent"),
        Some(vec![(
            fixture.target_thread_id,
            AgentStatus::Completed(Some("completed after release".into())),
        )])
    );
    assert!(*parent_revisions.borrow_and_update() > revision);
    assert_eq!(
        fixture
            .parent_thread
            .session
            .input_queue
            .join_owner_turn_for_generation(fixture.parent_thread_id, 1)
            .await,
        Ok(Some("A0".into()))
    );
    assert_eq!(
        fixture
            .parent_thread
            .session
            .active_turn
            .lock()
            .await
            .as_ref()
            .and_then(|turn| turn.task.as_ref())
            .map(|task| task.turn_context.sub_id.clone()),
        Some("A0".into())
    );
    let target_thread_id = fixture.target_thread_id.to_string();
    let (baseline_input, _) = fixture
        .parent_thread
        .session
        .input_queue
        .preview_mailbox_input_items()
        .await;
    let completed_aggregate_baseline = baseline_input
        .iter()
        .filter(|input| {
            matches!(
                input,
                TurnInput::InterAgentCommunication(communication)
                    if communication.trigger_turn
                        && communication.content.contains(&target_thread_id)
                        && communication.content.contains("Completed")
            )
        })
        .count();
    b6_enqueue_join(&fixture.target_thread, "B1", inner_generation).await;
    let (_, b2_state, release_b2) = b6_gated_turn(&fixture.target_thread, "B2").await;
    let Some((observed_generation, Some((successor_turn_id, leased_state, terminal_status)))) =
        fixture
            .target_thread
            .session
            .input_queue
            .join_continuation_for_predecessor(fixture.target_thread_id, "B1")
            .await
            .expect("B1 continuation lookup")
    else {
        panic!("real B2 startup should install the exact B1 continuation lease");
    };
    assert_eq!(observed_generation, inner_generation);
    assert_eq!(successor_turn_id, "B2");
    assert!(Arc::ptr_eq(&leased_state, &b2_state));
    assert_eq!(terminal_status, None);
    drop(leased_state);
    release_b2.notify_one();
    timeout(Duration::from_secs(2), async {
        while fixture
            .target_thread
            .session
            .active_turn
            .lock()
            .await
            .is_some()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("B2 should complete and become idle");
    timeout(Duration::from_secs(2), async {
        loop {
            let (input, _) = fixture
                .parent_thread
                .session
                .input_queue
                .preview_mailbox_input_items()
                .await;
            let completed_aggregate_count = input
                .iter()
                .filter(|input| {
                    matches!(
                        input,
                        TurnInput::InterAgentCommunication(communication)
                            if communication.trigger_turn
                                && communication.content.contains(&target_thread_id)
                                && communication.content.contains("Completed")
                    )
                })
                .count();
            let continuation_retired = fixture
                .target_thread
                .session
                .input_queue
                .join_continuation_for_predecessor(fixture.target_thread_id, "B1")
                .await
                .expect("B1 continuation lookup should stay unambiguous")
                .is_none();
            if completed_aggregate_count > completed_aggregate_baseline && continuation_retired {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("B2 terminal should aggregate to the unchanged A0 owner and retire the B1 lease");
}

#[path = "../join_observer_lag_tests.rs"]
mod join_observer_lag_tests;
