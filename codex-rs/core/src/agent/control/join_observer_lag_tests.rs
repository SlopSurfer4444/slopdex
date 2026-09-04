use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_join_observer_recovers_target_terminal_after_broadcast_lag() {
    let harness = AgentControlHarness::new().await;
    let (parent_id, parent) = harness.start_thread().await;
    harness.control.register_session_root(parent_id, None);
    let parent_turn = parent
        .session
        .new_turn_with_default_settings("A0".into(), Default::default())
        .await;
    parent
        .session
        .start_task(parent_turn, Vec::new(), NeverEndingCapacityTask)
        .await;
    let (child_id, child) =
        b6_spawn_pathful_child(&harness, parent_id, &parent, "A0", "lagged_child").await;
    child
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    let (_, child_turn_state, release_child_turn) = b6_gated_turn(&child, "B1").await;

    let observer_entered = Arc::new(tokio::sync::Notify::new());
    let release_observer = Arc::new(tokio::sync::Notify::new());
    harness
        .control
        .set_exact_join_observer_receive_barrier(
            Arc::clone(&observer_entered),
            Arc::clone(&release_observer),
        )
        .await;
    assert!(
        harness
            .control
            .register_join_obligation(parent_id, "A0".into(), vec![child_id])
            .await
            .expect("join should register")
    );
    timeout(Duration::from_secs(2), observer_entered.notified())
        .await
        .expect("join observer should pause before its first receive");

    child
        .session
        .input_queue
        .record_nested_join_turn_terminal(
            child_id,
            "B1",
            &child_turn_state,
            AgentStatus::Completed(Some("target terminal".into())),
        )
        .await;
    for index in 0..65 {
        child
            .session
            .input_queue
            .record_nested_join_turn_terminal(
                child_id,
                &format!("noise-{index}"),
                &child_turn_state,
                AgentStatus::Completed(None),
            )
            .await;
    }
    release_observer.notify_one();

    timeout(Duration::from_secs(2), async {
        loop {
            let (pending, _) = parent
                .session
                .input_queue
                .preview_mailbox_input_items()
                .await;
            if pending.iter().any(|input| {
                matches!(
                    input,
                    TurnInput::InterAgentCommunication(communication)
                        if communication.trigger_turn
                            && communication.content.contains(&child_id.to_string())
                )
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("lagged observer should recover the exact terminal from authoritative state");

    release_child_turn.notify_one();
    b6_wait_status(
        &child,
        AgentStatus::Completed(Some("completed after release".into())),
    )
    .await;
    tokio::task::yield_now().await;
    let (pending, _) = parent
        .session
        .input_queue
        .preview_mailbox_input_items()
        .await;
    assert!(
        pending
            .iter()
            .filter(|input| {
                matches!(
                    input,
                    TurnInput::InterAgentCommunication(communication)
                        if communication.trigger_turn
                            && communication.content.contains(&child_id.to_string())
                )
            })
            .count()
            == 1,
        "the authoritative recovery and later duplicate terminal must resolve once"
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
