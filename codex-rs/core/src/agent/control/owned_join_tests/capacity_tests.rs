use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn b5_capacity_ready_lease_reenters_native_pending_work_after_release() {
    let (home, mut config) = test_config_with_cli_overrides(vec![(
        "agents.max_threads".to_string(),
        TomlValue::Integer(2),
    )])
    .await;
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("test config should allow feature update");
    let harness = AgentControlHarness::new_with_config(home, config.clone()).await;
    let (root_thread_id, root_thread) = harness.start_thread().await;
    let parent_source = thread_spawn_source(
        root_thread_id,
        &root_thread.session_source,
        next_thread_spawn_depth(&root_thread.session_source),
        /*agent_role*/ None,
        Some("parent".to_string()),
    )
    .expect("parent source should carry an agent path");
    let parent_thread_id = harness
        .control
        .spawn_agent_with_metadata(
            harness.config.clone(),
            text_input("parent task"),
            Some(parent_source),
            SpawnAgentOptions {
                parent_thread_id: Some(root_thread_id),
                ..Default::default()
            },
        )
        .await
        .expect("parent child should start")
        .thread_id;
    let parent_thread = harness
        .manager
        .get_thread(parent_thread_id)
        .await
        .expect("parent child should remain addressable");
    assert!(
        harness
            .control
            .get_agent_metadata(parent_thread_id)
            .and_then(|metadata| metadata.agent_path)
            .is_some(),
        "parent metadata must carry an agent path"
    );
    let target_source = thread_spawn_source(
        parent_thread_id,
        &parent_thread.session_source,
        next_thread_spawn_depth(&parent_thread.session_source),
        /*agent_role*/ None,
        Some("target".to_string()),
    )
    .expect("target source should carry an agent path");
    let target_thread_id = harness
        .control
        .spawn_agent_with_metadata(
            harness.config.clone(),
            text_input("target task"),
            Some(target_source),
            SpawnAgentOptions {
                parent_thread_id: Some(parent_thread_id),
                ..Default::default()
            },
        )
        .await
        .expect("target child should start")
        .thread_id;
    let target_thread = harness
        .manager
        .get_thread(target_thread_id)
        .await
        .expect("target child should remain addressable");
    assert!(
        harness
            .control
            .get_agent_metadata(target_thread_id)
            .and_then(|metadata| metadata.agent_path)
            .is_some(),
        "target metadata must carry an agent path"
    );

    // Replace the spawned bootstrap task with an in-crate task that remains genuinely active,
    // so registration captures a real RunningTask/current turn instead of relying on status.
    parent_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    let target_turn = target_thread.session.new_default_turn().await;
    target_thread
        .session
        .start_task(target_turn, Vec::new(), NeverEndingCapacityTask)
        .await;
    assert!(
        target_thread
            .session
            .active_turn
            .lock()
            .await
            .as_ref()
            .is_some_and(|turn| turn.task.is_some())
    );

    // Leave the parent idle while retaining an unrelated guard and the target's real guard,
    // making the parent's next pending turn genuinely capacity-saturated.
    let saturated_guard = harness
        .control
        .reserve_execution_capacity(
            MultiAgentVersion::V2,
            &SessionSource::SubAgent(SubAgentSource::Other("capacity-holder".to_string())),
        )
        .expect("the released parent slot should be held for saturation");
    let parent_turn_id = "b5-capacity-parent-turn".to_string();
    let registered = harness
        .control
        .register_join_obligation(
            parent_thread_id,
            parent_turn_id.clone(),
            vec![target_thread_id],
        )
        .await
        .expect("join registration should reach the manager-owned parent");
    assert!(registered);
    let barrier = PendingWakeClaimBarrier::after_commit();
    harness
        .control
        .set_capacity_ready_barrier(barrier.clone())
        .await;
    let (mut activity_rx, pending_activity) = parent_thread
        .session
        .input_queue
        .subscribe_activity(None)
        .await;

    let _terminal_capacity_guard =
        shutdown_terminal_while_preserving_saturation(&harness, &target_thread).await;
    if pending_activity.is_none() {
        activity_rx
            .changed()
            .await
            .expect("saturated terminal join should enqueue its retained trigger");
    }
    assert!(
        parent_thread
            .session
            .input_queue
            .has_trigger_turn_mailbox_items()
            .await
    );
    assert_eq!(harness.control.capacity_ready_leases.lock().await.len(), 1);

    drop(saturated_guard);
    barrier.wait_until_claimed().await;
    {
        let active_turn = parent_thread.session.active_turn.lock().await;
        let active_turn = active_turn
            .as_ref()
            .expect("capacity release should claim the native pending turn");
        assert!(active_turn.task.is_none());
    }
    assert!(
        parent_thread
            .session
            .input_queue
            .has_pending_join_generation(
                harness
                    .control
                    .capacity_ready_leases
                    .lock()
                    .await
                    .first()
                    .expect("lease should remain until exact consumption")
                    .generation,
            )
            .await
    );
    barrier.release();

    parent_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b5_capacity_ready_lease_retires_after_exact_parent_removal() {
    let fixture = b5_capacity_fixture().await;
    let harness = &fixture.harness;
    let saturated_guard = harness
        .control
        .reserve_execution_capacity(
            MultiAgentVersion::V2,
            &SessionSource::SubAgent(SubAgentSource::Other("capacity-holder".to_string())),
        )
        .expect("the released parent slot should be held for saturation");
    let parent_turn_id = "b5-capacity-parent-retired".to_string();
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                parent_turn_id.clone(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("join registration should reach the manager-owned parent")
    );
    let (mut activity_rx, pending_activity) = fixture
        .parent_thread
        .session
        .input_queue
        .subscribe_activity(None)
        .await;
    let _terminal_capacity_guard =
        shutdown_terminal_while_preserving_saturation(harness, &fixture.target_thread).await;
    if pending_activity.is_none() {
        activity_rx
            .changed()
            .await
            .expect("terminal target should enqueue its retained trigger");
    }
    let generation = harness
        .control
        .capacity_ready_leases
        .lock()
        .await
        .first()
        .map(|lease| lease.generation)
        .unwrap_or_else(|| panic!("terminal join should retain a capacity lease"));
    let retired = harness
        .control
        .watch_capacity_ready_lease_retirement(
            fixture.parent_thread_id,
            parent_turn_id,
            generation,
            Arc::clone(&fixture.parent_thread),
        )
        .await;
    assert!(
        harness
            .manager
            .remove_thread(&fixture.parent_thread_id)
            .await
            .is_some(),
        "the exact parent runtime should be removed"
    );
    drop(saturated_guard);
    retired.notified().await;
    assert!(
        harness
            .control
            .capacity_ready_leases
            .lock()
            .await
            .is_empty()
    );

    fixture
        .target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b5_capacity_ready_lease_retires_without_waking_same_id_replacement() {
    let root_id = ThreadId::new();
    let parent_id = ThreadId::new();
    let target_id = ThreadId::new();
    let ids = Arc::new(std::sync::Mutex::new(
        vec![root_id, parent_id, target_id, parent_id].into_iter(),
    ));
    let generator_ids = Arc::clone(&ids);
    let fixture = b5_capacity_fixture_with_limit_and_generator(2, move || {
        generator_ids
            .lock()
            .expect("test thread id generator lock")
            .next()
            .expect("test thread id sequence should be complete")
    })
    .await;
    assert_eq!(fixture.parent_thread_id, parent_id);
    assert_eq!(fixture.target_thread_id, target_id);
    let harness = &fixture.harness;
    let saturated_guard = harness
        .control
        .reserve_execution_capacity(
            MultiAgentVersion::V2,
            &SessionSource::SubAgent(SubAgentSource::Other("capacity-holder".to_string())),
        )
        .expect("the released parent slot should be held for saturation");
    let parent_turn_id = "b5-capacity-parent-replacement".to_string();
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                parent_turn_id.clone(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("join registration should reach the manager-owned parent")
    );
    let (mut activity_rx, pending_activity) = fixture
        .parent_thread
        .session
        .input_queue
        .subscribe_activity(None)
        .await;
    let _terminal_capacity_guard =
        shutdown_terminal_while_preserving_saturation(harness, &fixture.target_thread).await;
    if pending_activity.is_none() {
        activity_rx
            .changed()
            .await
            .expect("terminal target should enqueue its retained trigger");
    }
    let generation = harness
        .control
        .capacity_ready_leases
        .lock()
        .await
        .first()
        .map(|lease| lease.generation)
        .expect("terminal join should retain a capacity lease");
    let retired = harness
        .control
        .watch_capacity_ready_lease_retirement(
            fixture.parent_thread_id,
            parent_turn_id,
            generation,
            Arc::clone(&fixture.parent_thread),
        )
        .await;
    fixture
        .parent_thread
        .shutdown_and_wait()
        .await
        .expect("old parent shutdown should complete");
    assert!(
        harness
            .manager
            .remove_thread(&fixture.parent_thread_id)
            .await
            .is_some(),
        "the old parent runtime should be removed"
    );
    let replacement = harness
        .manager
        .start_thread(StartThreadOptions::new(harness.config.clone()))
        .await
        .expect("same-id replacement should start");
    assert_eq!(replacement.thread_id, fixture.parent_thread_id);
    assert!(!Arc::ptr_eq(&replacement.thread, &fixture.parent_thread));
    drop(saturated_guard);
    retired.notified().await;
    assert!(
        harness
            .control
            .capacity_ready_leases
            .lock()
            .await
            .is_empty()
    );
    assert!(
        replacement
            .thread
            .session
            .active_turn
            .lock()
            .await
            .is_none()
    );

    replacement
        .thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    fixture
        .target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b5_cancelled_startup_commit_preserves_join_for_later_winner() {
    let fixture = b5_capacity_fixture().await;
    let harness = &fixture.harness;
    let saturated_guard = harness
        .control
        .reserve_execution_capacity(
            MultiAgentVersion::V2,
            &SessionSource::SubAgent(SubAgentSource::Other("capacity-holder".to_string())),
        )
        .expect("the released parent slot should be held for saturation");
    let parent_turn_id = "b5-cancelled-startup-commit".to_string();
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                parent_turn_id.clone(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("join registration should reach the manager-owned parent")
    );
    let (mut activity_rx, pending_activity) = fixture
        .parent_thread
        .session
        .input_queue
        .subscribe_activity(None)
        .await;
    let _terminal_capacity_guard =
        shutdown_terminal_while_preserving_saturation(harness, &fixture.target_thread).await;
    if pending_activity.is_none() {
        activity_rx
            .changed()
            .await
            .expect("terminal target should enqueue its retained trigger");
    }
    let generation = harness
        .control
        .capacity_ready_leases
        .lock()
        .await
        .iter()
        .find(|lease| {
            lease.parent_thread_id == fixture.parent_thread_id
                && lease.parent_turn_id == parent_turn_id
                && Arc::ptr_eq(&lease.parent_thread, &fixture.parent_thread)
        })
        .map(|lease| lease.generation)
        .expect("terminal join should retain the exact capacity lease");
    let automatic_barrier = PendingWakeClaimBarrier::new();
    harness
        .control
        .set_capacity_ready_barrier(automatic_barrier.clone())
        .await;
    drop(saturated_guard);
    automatic_barrier.wait_until_claimed().await;
    let retired = harness
        .control
        .watch_capacity_ready_lease_retirement(
            fixture.parent_thread_id,
            parent_turn_id.clone(),
            generation,
            Arc::clone(&fixture.parent_thread),
        )
        .await;

    let UserTurnStartClaim::Claimed {
        reservation,
        turn_state: cancelled_turn_state,
    } = fixture
        .parent_thread
        .session
        .claim_execution_capacity_for_user_turn_start()
        .await
        .expect("the explicit start should transfer the pending wake reservation")
    else {
        panic!("the explicit start should own the transferred reservation");
    };
    let cancelled_turn_context = fixture.parent_thread.session.new_default_turn().await;
    let commit_barrier = TaskStartCommitBarrier::new();
    let parent = Arc::clone(&fixture.parent_thread);
    let commit_barrier_for_task = commit_barrier.clone();
    let cancelled_start = tokio::spawn(async move {
        parent
            .session
            .start_task_with_reservation_and_barrier(
                cancelled_turn_context,
                Vec::new(),
                NeverEndingCapacityTask,
                reservation,
                commit_barrier_for_task,
            )
            .await;
    });
    commit_barrier.wait_until_reached().await;
    cancelled_start.abort();
    assert!(
        cancelled_start
            .await
            .expect_err("the hostile start should abort")
            .is_cancelled(),
        "the hostile start should be cancelled at the pre-commit barrier"
    );
    assert!(
        fixture
            .parent_thread
            .session
            .input_queue
            .has_trigger_turn_mailbox_items()
            .await,
        "a pre-commit cancellation must leave the join trigger in the mailbox"
    );
    assert!(
        fixture
            .parent_thread
            .session
            .input_queue
            .has_pending_join_generation(generation)
            .await,
        "a pre-commit cancellation must leave the exact generation live"
    );
    assert_eq!(harness.control.capacity_ready_leases.lock().await.len(), 1);
    assert!(
        fixture
            .parent_thread
            .session
            .input_queue
            .take_pending_input_for_turn_state(cancelled_turn_state.as_ref())
            .await
            .is_empty(),
        "the cancelled claimant must not receive join material"
    );

    let UserTurnStartClaim::Claimed {
        reservation,
        turn_state: winning_turn_state,
    } = fixture
        .parent_thread
        .session
        .claim_execution_capacity_for_user_turn_start()
        .await
        .expect("a later explicit start should claim released capacity")
    else {
        panic!("a later explicit start should become the winner");
    };
    let winning_turn_context = fixture.parent_thread.session.new_default_turn().await;
    fixture
        .parent_thread
        .session
        .start_task_with_reservation(
            winning_turn_context,
            Vec::new(),
            NeverEndingCapacityTask,
            reservation,
        )
        .await;
    let pending_input = fixture
        .parent_thread
        .session
        .input_queue
        .take_pending_input_for_turn_state(winning_turn_state.as_ref())
        .await;
    assert_matches!(
        pending_input.as_slice(),
        [TurnInput::InterAgentCommunication(communication)]
            if communication.trigger_turn && communication.content.contains(&parent_turn_id)
    );
    assert!(
        !fixture
            .parent_thread
            .session
            .input_queue
            .has_pending_join_generation(generation)
            .await
    );
    automatic_barrier.release();
    retired.notified().await;
    assert!(
        harness
            .control
            .capacity_ready_leases
            .lock()
            .await
            .is_empty()
    );

    fixture
        .parent_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    fixture
        .target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b5_replaced_startup_turn_state_rejects_loser_installation() {
    let harness = AgentControlHarness::new().await;
    let (_, thread) = harness.start_thread().await;
    let communication = InterAgentCommunication::new(
        AgentPath::root(),
        AgentPath::try_from("/root/worker").expect("agent path"),
        Vec::new(),
        "replacement probe".to_string(),
        /*trigger_turn*/ true,
    );
    thread
        .session
        .input_queue
        .enqueue_mailbox_communication(communication, Default::default())
        .await;
    let UserTurnStartClaim::Claimed {
        reservation,
        turn_state: original_turn_state,
    } = thread
        .session
        .claim_execution_capacity_for_user_turn_start()
        .await
        .expect("the first start should claim capacity")
    else {
        panic!("the first start should own a taskless claim");
    };
    let original_claim = thread
        .session
        .active_turn
        .lock()
        .await
        .as_ref()
        .and_then(ActiveTurn::taskless_start_claim)
        .expect("the first start should install its claim");
    let turn_context = thread.session.new_default_turn().await;
    let commit_barrier = TaskStartCommitBarrier::new();
    let thread_for_start = Arc::clone(&thread);
    let commit_barrier_for_task = commit_barrier.clone();
    let losing_start = tokio::spawn(async move {
        thread_for_start
            .session
            .start_task_with_reservation_and_barrier(
                turn_context,
                Vec::new(),
                NeverEndingCapacityTask,
                reservation,
                commit_barrier_for_task,
            )
            .await;
    });
    commit_barrier.wait_until_reached().await;
    let replacement = ActiveTurn::with_taskless_start_claim(original_claim);
    let replacement_turn_state = Arc::clone(&replacement.turn_state);
    *thread.session.active_turn.lock().await = Some(replacement);
    commit_barrier.release();
    losing_start
        .await
        .expect("the rejected start task should return cleanly");

    {
        let active = thread.session.active_turn.lock().await;
        let active = active
            .as_ref()
            .expect("the replacement should remain active");
        assert!(Arc::ptr_eq(&active.turn_state, &replacement_turn_state));
        assert!(
            active.task.is_none(),
            "the losing start must not install into a replacement turn state"
        );
    }
    assert!(
        thread
            .session
            .input_queue
            .has_trigger_turn_mailbox_items()
            .await,
        "the losing start must leave mailbox material for the replacement"
    );
    assert!(
        thread
            .session
            .input_queue
            .take_pending_input_for_turn_state(original_turn_state.as_ref())
            .await
            .is_empty(),
        "the losing start must not attach material to its superseded turn state"
    );

    thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b5_capacity_ready_lease_dedupes_duplicate_terminal_and_release_signals() {
    let fixture = b5_capacity_fixture_with_limit(3).await;
    let harness = &fixture.harness;
    let first_guard = harness
        .control
        .reserve_execution_capacity(
            MultiAgentVersion::V2,
            &SessionSource::SubAgent(SubAgentSource::Other("capacity-holder-1".to_string())),
        )
        .expect("first capacity holder should reserve");
    let second_guard = harness
        .control
        .reserve_execution_capacity(
            MultiAgentVersion::V2,
            &SessionSource::SubAgent(SubAgentSource::Other("capacity-holder-2".to_string())),
        )
        .expect("second capacity holder should reserve");
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                "b5-duplicate-signals".to_string(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("join registration should reach the manager-owned parent")
    );
    let (mut activity_rx, pending_activity) = fixture
        .parent_thread
        .session
        .input_queue
        .subscribe_activity(None)
        .await;
    let _terminal_capacity_guard =
        shutdown_terminal_while_preserving_saturation(harness, &fixture.target_thread).await;
    fixture
        .target_thread
        .submit(Op::Shutdown {})
        .await
        .expect("duplicate terminal target shutdown should submit");
    if pending_activity.is_none() {
        activity_rx
            .changed()
            .await
            .expect("terminal target should enqueue its retained trigger");
    }
    let generation = harness
        .control
        .capacity_ready_leases
        .lock()
        .await
        .iter()
        .find(|lease| {
            lease.parent_thread_id == fixture.parent_thread_id
                && lease.parent_turn_id == "b5-duplicate-signals"
                && Arc::ptr_eq(&lease.parent_thread, &fixture.parent_thread)
        })
        .map(|lease| lease.generation)
        .expect("duplicate completion should retain one exact capacity lease");
    let retired = harness
        .control
        .watch_capacity_ready_lease_retirement(
            fixture.parent_thread_id,
            "b5-duplicate-signals".to_string(),
            generation,
            Arc::clone(&fixture.parent_thread),
        )
        .await;
    let barrier = PendingWakeClaimBarrier::after_commit();
    harness
        .control
        .set_capacity_ready_barrier(barrier.clone())
        .await;
    drop(first_guard);
    barrier.wait_until_claimed().await;
    drop(second_guard);
    assert_eq!(harness.control.capacity_ready_leases.lock().await.len(), 1);
    assert!(
        fixture
            .parent_thread
            .session
            .active_turn
            .lock()
            .await
            .as_ref()
            .is_some_and(|turn| turn.task.is_none())
    );
    barrier.release();

    timeout(Duration::from_secs(5), async {
        loop {
            if fixture
                .parent_thread
                .session
                .active_turn
                .lock()
                .await
                .as_ref()
                .is_some_and(|turn| turn.task.is_some())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the retained wake should install one task");
    let turn_state = {
        let active = fixture.parent_thread.session.active_turn.lock().await;
        Arc::clone(
            &active
                .as_ref()
                .expect("the retained wake should remain active")
                .turn_state,
        )
    };
    let pending_input = fixture
        .parent_thread
        .session
        .input_queue
        .take_pending_input_for_turn_state(turn_state.as_ref())
        .await;
    assert_matches!(
        pending_input.as_slice(),
        [TurnInput::InterAgentCommunication(communication)]
            if communication.trigger_turn
                && communication.content.contains("b5-duplicate-signals")
    );
    assert!(
        !fixture
            .parent_thread
            .session
            .input_queue
            .has_pending_join_generation(generation)
            .await
    );
    retired.notified().await;
    assert!(
        harness
            .control
            .capacity_ready_leases
            .lock()
            .await
            .is_empty()
    );
    assert!(
        fixture.target_thread.submit(Op::Shutdown {}).await.is_err(),
        "a post-terminal duplicate shutdown should be rejected"
    );
    tokio::task::yield_now().await;
    assert!(
        !fixture
            .parent_thread
            .session
            .input_queue
            .has_pending_mailbox_items()
            .await
    );

    fixture
        .parent_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    fixture
        .target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b5_join_registration_fences_duplicate_and_preserves_new_generation() {
    let fixture = b5_capacity_fixture().await;
    let harness = &fixture.harness;
    let first_turn = "b5-generation-one".to_string();
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                first_turn.clone(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("first generation should register")
    );
    assert!(
        !harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                first_turn,
                vec![fixture.target_thread_id],
            )
            .await
            .expect("duplicate generation should be rejected deterministically")
    );
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                "b5-generation-two".to_string(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("a distinct current parent generation should remain registerable")
    );

    fixture
        .parent_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    fixture
        .target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}

#[tokio::test]
async fn b5_user_start_claim_transfers_pending_join_wake_exactly_once() {
    let fixture = b5_capacity_fixture().await;
    let harness = &fixture.harness;
    let saturated_guard = harness
        .control
        .reserve_execution_capacity(
            MultiAgentVersion::V2,
            &SessionSource::SubAgent(SubAgentSource::Other("capacity-holder".to_string())),
        )
        .expect("the released parent slot should be held for saturation");
    let parent_turn_id = "b5-capacity-user-priority".to_string();
    assert!(
        harness
            .control
            .register_join_obligation(
                fixture.parent_thread_id,
                parent_turn_id.clone(),
                vec![fixture.target_thread_id],
            )
            .await
            .expect("join registration should reach the manager-owned parent")
    );
    let (mut activity_rx, pending_activity) = fixture
        .parent_thread
        .session
        .input_queue
        .subscribe_activity(None)
        .await;
    let _terminal_capacity_guard =
        shutdown_terminal_while_preserving_saturation(harness, &fixture.target_thread).await;
    if pending_activity.is_none() {
        activity_rx
            .changed()
            .await
            .expect("terminal target should enqueue its retained trigger");
    }
    let generation = harness
        .control
        .capacity_ready_leases
        .lock()
        .await
        .iter()
        .find(|lease| {
            lease.parent_thread_id == fixture.parent_thread_id
                && lease.parent_turn_id == parent_turn_id
                && Arc::ptr_eq(&lease.parent_thread, &fixture.parent_thread)
        })
        .map(|lease| lease.generation)
        .expect("terminal join should retain the exact capacity lease");
    let automatic_barrier = PendingWakeClaimBarrier::new();
    harness
        .control
        .set_capacity_ready_barrier(automatic_barrier.clone())
        .await;
    drop(saturated_guard);
    automatic_barrier.wait_until_claimed().await;
    let retired = harness
        .control
        .watch_capacity_ready_lease_retirement(
            fixture.parent_thread_id,
            parent_turn_id.clone(),
            generation,
            Arc::clone(&fixture.parent_thread),
        )
        .await;

    let user_barrier = crate::session::session::Session::new_user_start_claim_barrier();
    let parent = Arc::clone(&fixture.parent_thread);
    let user_barrier_for_task = user_barrier.clone();
    let user_start = tokio::spawn(async move {
        parent
            .session
            .start_or_steer_with_task_and_barrier(
                TurnInputRequest::user_input(text_input("explicit user priority")),
                "b5-user-priority-turn".to_string(),
                NeverEndingCapacityTask,
                user_barrier_for_task,
            )
            .await
    });
    user_barrier.wait_until_claimed().await;
    assert!(
        fixture
            .parent_thread
            .session
            .active_turn
            .lock()
            .await
            .as_ref()
            .is_some_and(|turn| turn.task.is_none())
    );
    assert!(
        fixture
            .parent_thread
            .session
            .input_queue
            .has_trigger_turn_mailbox_items()
            .await
    );
    user_barrier.release();
    assert_matches!(
        user_start
            .await
            .expect("user start task should join")
            .expect("user start should succeed"),
        TurnInputSubmission::Started { .. }
    );
    let turn_state = {
        let active = fixture.parent_thread.session.active_turn.lock().await;
        let turn = active
            .as_ref()
            .expect("the winning user turn should remain active");
        assert_eq!(
            turn.task
                .as_ref()
                .map(|task| task.turn_context.sub_id.as_str()),
            Some("b5-user-priority-turn")
        );
        Arc::clone(&turn.turn_state)
    };
    let pending_input = fixture
        .parent_thread
        .session
        .input_queue
        .take_pending_input_for_turn_state(turn_state.as_ref())
        .await;
    assert_matches!(
        pending_input.as_slice(),
        [TurnInput::InterAgentCommunication(communication)]
            if communication.trigger_turn && communication.content.contains(&parent_turn_id)
    );
    assert!(
        !fixture
            .parent_thread
            .session
            .input_queue
            .has_pending_join_generation(generation)
            .await
    );
    automatic_barrier.release();
    retired.notified().await;
    assert!(
        harness
            .control
            .capacity_ready_leases
            .lock()
            .await
            .is_empty()
    );

    assert!(
        fixture.target_thread.submit(Op::Shutdown {}).await.is_err(),
        "a post-terminal duplicate shutdown should be rejected"
    );
    tokio::task::yield_now().await;
    assert!(
        !fixture
            .parent_thread
            .session
            .input_queue
            .has_pending_mailbox_items()
            .await
    );
    assert!(
        harness
            .control
            .capacity_ready_leases
            .lock()
            .await
            .is_empty()
    );

    fixture
        .parent_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    tokio::task::yield_now().await;
    assert!(
        fixture
            .parent_thread
            .session
            .active_turn
            .lock()
            .await
            .is_none()
    );
    assert!(
        harness
            .control
            .capacity_ready_leases
            .lock()
            .await
            .is_empty()
    );
    fixture
        .target_thread
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
}
