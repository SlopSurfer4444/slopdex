use super::*;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use pretty_assertions::assert_eq;
use std::any::Any;
use std::collections::HashMap;

fn make_mail(
    author: AgentPath,
    recipient: AgentPath,
    content: &str,
    trigger_turn: bool,
) -> InterAgentCommunication {
    InterAgentCommunication::new(
        author,
        recipient,
        Vec::new(),
        content.to_string(),
        trigger_turn,
    )
}

fn join_targets(
    child_ids: &[ThreadId],
    turn_ids: &[&str],
) -> (
    HashMap<ThreadId, (Arc<dyn Any + Send + Sync>, String)>,
    Vec<Arc<dyn Any + Send + Sync>>,
) {
    let incarnations = child_ids
        .iter()
        .map(|_| Arc::new(()) as Arc<dyn Any + Send + Sync>)
        .collect::<Vec<_>>();
    let targets = child_ids
        .iter()
        .zip(turn_ids.iter())
        .zip(incarnations.iter())
        .map(|((child_id, turn_id), incarnation)| {
            (*child_id, (incarnation.clone(), (*turn_id).to_string()))
        })
        .collect::<HashMap<_, _>>();
    (targets, incarnations)
}

#[tokio::test]
async fn multi_agent_v2_join_agents_defers_until_parent_turn_is_idle() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let (targets, incarnations) = join_targets(&[child], &["child-turn"]);
    let obligation = input_queue
        .register_join_obligation(parent, "parent-turn".to_string(), targets)
        .await
        .expect("join registration should be owned by the parent queue");

    assert!(
        !input_queue
            .consume_ready_join_obligation("parent-turn", obligation.generation())
            .await
    );
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                obligation.generation(),
                child,
                &incarnations[0],
                "child-turn",
                AgentStatus::Completed(None),
            )
            .await
            .is_some()
    );
    assert!(
        input_queue
            .consume_ready_join_obligation("parent-turn", obligation.generation())
            .await
    );
    assert!(
        !input_queue
            .consume_ready_join_obligation("parent-turn", obligation.generation())
            .await
    );
}

#[tokio::test]
async fn b5_input_queue_consumes_each_provenance_bound_join_in_mixed_mailbox() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let first_child = ThreadId::new();
    let second_child = ThreadId::new();
    let (first_targets, first_incarnations) = join_targets(&[first_child], &["first-turn"]);
    let (second_targets, second_incarnations) = join_targets(&[second_child], &["second-turn"]);
    let first = input_queue
        .register_join_obligation(parent, "first-parent-turn".to_string(), first_targets)
        .await
        .expect("first join should register");
    let second = input_queue
        .register_join_obligation(parent, "second-parent-turn".to_string(), second_targets)
        .await
        .expect("second join should register");

    for (parent_turn_id, child, child_turn_id, incarnation, generation) in [
        (
            "first-parent-turn",
            first_child,
            "first-turn",
            &first_incarnations[0],
            first.generation(),
        ),
        (
            "second-parent-turn",
            second_child,
            "second-turn",
            &second_incarnations[0],
            second.generation(),
        ),
    ] {
        assert!(
            input_queue
                .resolve_join_target(
                    parent,
                    generation,
                    child,
                    incarnation,
                    child_turn_id,
                    AgentStatus::Completed(None),
                )
                .await
                .is_some()
        );
        input_queue
            .mark_join_trigger(parent_turn_id, generation)
            .await;
        input_queue
            .enqueue_mailbox_communication(
                make_mail(
                    AgentPath::try_from("/root/worker").expect("agent path"),
                    AgentPath::root(),
                    parent_turn_id,
                    /*trigger_turn*/ true,
                ),
                TurnStartOptions {
                    parent_turn_id: Some(parent_turn_id.to_string()),
                    ..Default::default()
                },
            )
            .await;
    }

    let active_turn = Mutex::new(None);
    let (input, start_options) = input_queue.get_pending_input(&active_turn).await;
    assert_eq!(input.len(), 2);
    assert_eq!(start_options.parent_turn_id, None);
    assert!(
        !input_queue
            .has_pending_join_generation(first.generation())
            .await
    );
    assert!(
        !input_queue
            .has_pending_join_generation(second.generation())
            .await
    );
}

#[tokio::test]
async fn multi_agent_v2_targetful_wait_consumes_shared_retained_result() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let (targets, incarnations) = join_targets(&[child], &["child-turn"]);
    let current_targets = targets.clone();
    let terminal_obligation = input_queue
        .register_wait_obligation(parent, "wait-turn".to_string(), targets)
        .await
        .expect("join registration should succeed");
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                terminal_obligation.generation(),
                child,
                &incarnations[0],
                "child-turn",
                AgentStatus::Completed(None),
            )
            .await
            .is_some()
    );
    input_queue
        .finish_join_dispatch(terminal_obligation.generation())
        .await;

    assert!(
        input_queue
            .consume_ready_join_obligation_for_targets(
                parent,
                "wait-turn",
                &[child],
                &current_targets,
            )
            .await
    );
    assert!(
        !input_queue
            .consume_ready_join_obligation_for_targets(
                parent,
                "wait-turn",
                &[child],
                &current_targets,
            )
            .await
    );
}

#[tokio::test]
async fn multi_agent_v2_targetful_wait_consumes_retained_failed_join() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let (targets, incarnations) = join_targets(&[child], &["failed-turn"]);
    let current_targets = targets.clone();
    let join = input_queue
        .register_join_obligation(parent, "same-turn".to_string(), targets)
        .await
        .expect("join registration should succeed");
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                join.generation(),
                child,
                &incarnations[0],
                "failed-turn",
                AgentStatus::Errored("child failed".to_string()),
            )
            .await
            .is_some()
    );
    input_queue
        .mark_join_trigger("same-turn", join.generation())
        .await;
    let mut start_options = TurnStartOptions::default();
    start_options.parent_turn_id = Some("same-turn".to_string());
    input_queue
        .enqueue_mailbox_communication(
            make_mail(
                AgentPath::root(),
                AgentPath::root(),
                "retained join aggregate",
                true,
            ),
            start_options,
        )
        .await;

    let wait = input_queue
        .register_wait_obligation(parent, "same-turn".to_string(), current_targets.clone())
        .await
        .expect("wait should bind the retained join generation");
    assert_eq!(wait.generation(), join.generation());
    let outcomes = input_queue
        .consume_ready_join_obligation_for_targets_with_outcomes(
            parent,
            "same-turn",
            &[child],
            &current_targets,
        )
        .await
        .expect("targetful wait should consume the retained join");
    assert_eq!(
        outcomes,
        vec![(child, AgentStatus::Errored("child failed".to_string()))]
    );
    assert!(!input_queue.has_pending_mailbox_items().await);
    assert!(
        !input_queue
            .consume_ready_join_obligation_for_targets(
                parent,
                "same-turn",
                &[child],
                &current_targets,
            )
            .await
    );
}

#[tokio::test]
async fn multi_agent_v2_targetful_wait_timeout_retry_consumes_same_turn_completion() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let (targets, incarnations) = join_targets(&[child], &["retry-turn"]);
    let current_targets = targets.clone();
    let first_wait = input_queue
        .register_wait_obligation(parent, "retry-parent-turn".to_string(), targets)
        .await
        .expect("initial wait should register");

    // A timeout is non-terminal: the exact wait obligation remains owned
    // and a same-turn retry must reattach instead of registering a second
    // observer.
    let retry_wait = input_queue
        .register_wait_obligation(
            parent,
            "retry-parent-turn".to_string(),
            current_targets.clone(),
        )
        .await
        .expect("same-turn retry should reattach the retained wait");
    assert_eq!(retry_wait.generation(), first_wait.generation());
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                first_wait.generation(),
                child,
                &incarnations[0],
                "retry-turn",
                AgentStatus::Shutdown,
            )
            .await
            .is_some()
    );
    input_queue
        .finish_join_dispatch(first_wait.generation())
        .await;

    let outcomes = input_queue
        .consume_ready_join_obligation_for_targets_with_outcomes(
            parent,
            "retry-parent-turn",
            &[child],
            &current_targets,
        )
        .await
        .expect("retry should consume the exact completed wait");
    assert_eq!(outcomes, vec![(child, AgentStatus::Shutdown)]);
    assert!(
        !input_queue
            .consume_ready_join_obligation_for_targets(
                parent,
                "retry-parent-turn",
                &[child],
                &current_targets,
            )
            .await
    );
}

#[tokio::test]
async fn b6_ready_terminal_wait_accepts_ptr_equal_empty_current_turn() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let parent_turn_id = "b6-empty-current-turn";
    let (targets, incarnations) = join_targets(&[child], &["terminal-child-turn"]);
    let wait = input_queue
        .register_wait_obligation(parent, parent_turn_id.to_string(), targets)
        .await
        .expect("wait should register");
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                wait.generation(),
                child,
                &incarnations[0],
                "terminal-child-turn",
                AgentStatus::Shutdown,
            )
            .await
            .is_some()
    );
    input_queue.finish_join_dispatch(wait.generation()).await;
    let current_targets = HashMap::from([(child, (Arc::clone(&incarnations[0]), String::new()))]);

    assert_eq!(
        input_queue
            .consume_ready_join_obligation_for_targets_with_outcomes(
                parent,
                parent_turn_id,
                &[child],
                &current_targets,
            )
            .await,
        Some(vec![(child, AgentStatus::Shutdown)])
    );
}

#[tokio::test]
async fn b6_ready_terminal_wait_rejects_same_id_replacement_with_empty_turn() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let parent_turn_id = "b6-replacement-empty-turn";
    let (targets, incarnations) = join_targets(&[child], &["terminal-child-turn"]);
    let wait = input_queue
        .register_wait_obligation(parent, parent_turn_id.to_string(), targets)
        .await
        .expect("wait should register");
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                wait.generation(),
                child,
                &incarnations[0],
                "terminal-child-turn",
                AgentStatus::Shutdown,
            )
            .await
            .is_some()
    );
    input_queue.finish_join_dispatch(wait.generation()).await;
    let replacement: Arc<dyn Any + Send + Sync> = Arc::new(());
    let current_targets = HashMap::from([(child, (replacement, String::new()))]);

    assert!(
        input_queue
            .consume_ready_join_obligation_for_targets_with_outcomes(
                parent,
                parent_turn_id,
                &[child],
                &current_targets,
            )
            .await
            .is_none(),
        "a same-ID replacement Arc must not consume the original terminal result"
    );
}

#[tokio::test]
async fn b6_ready_terminal_wait_rejects_nonempty_mismatched_current_turn() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let parent_turn_id = "b6-mismatched-current-turn";
    let (targets, incarnations) = join_targets(&[child], &["terminal-child-turn"]);
    let wait = input_queue
        .register_wait_obligation(parent, parent_turn_id.to_string(), targets)
        .await
        .expect("wait should register");
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                wait.generation(),
                child,
                &incarnations[0],
                "terminal-child-turn",
                AgentStatus::Shutdown,
            )
            .await
            .is_some()
    );
    input_queue.finish_join_dispatch(wait.generation()).await;
    let current_targets = HashMap::from([(
        child,
        (
            Arc::clone(&incarnations[0]),
            "successor-child-turn".to_string(),
        ),
    )]);

    assert!(
        input_queue
            .consume_ready_join_obligation_for_targets_with_outcomes(
                parent,
                parent_turn_id,
                &[child],
                &current_targets,
            )
            .await
            .is_none(),
        "a nonempty successor turn must not consume the predecessor result"
    );
}

#[tokio::test]
async fn multi_agent_v2_join_agents_retains_failed_and_cancelled_outcomes() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let failed = ThreadId::new();
    let cancelled = ThreadId::new();
    let (targets, incarnations) =
        join_targets(&[failed, cancelled], &["failed-turn", "cancelled-turn"]);
    let terminal_obligation = input_queue
        .register_join_obligation(parent, "terminal-turn".to_string(), targets)
        .await
        .expect("join registration should succeed");

    assert!(
        input_queue
            .resolve_join_target(
                parent,
                terminal_obligation.generation(),
                failed,
                &incarnations[0],
                "failed-turn",
                AgentStatus::Errored("failed".to_string()),
            )
            .await
            .is_none()
    );
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                terminal_obligation.generation(),
                cancelled,
                &incarnations[1],
                "cancelled-turn",
                AgentStatus::Errored("Cancelled".to_string()),
            )
            .await
            .is_some()
    );
    assert!(
        input_queue
            .consume_ready_join_obligation("terminal-turn", terminal_obligation.generation())
            .await
    );
}

#[tokio::test]
async fn multi_agent_v2_join_agents_survives_handler_cancellation_and_fences_replacement() {
    let input_queue = InputQueue::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let (targets, incarnations) = join_targets(&[child], &["old-incarnation"]);
    let old_obligation = input_queue
        .register_join_obligation(parent, "replacement-turn".to_string(), targets)
        .await
        .expect("join registration should succeed");

    assert!(
        input_queue
            .resolve_join_target(
                parent,
                old_obligation.generation(),
                child,
                &incarnations[0],
                "replacement-incarnation",
                AgentStatus::Completed(None),
            )
            .await
            .is_none()
    );
    let (duplicate_targets, _) = join_targets(&[child], &["duplicate-incarnation"]);
    assert!(
        input_queue
            .register_join_obligation(parent, "replacement-turn".to_string(), duplicate_targets,)
            .await
            .is_none()
    );
    let (shutdown_targets, shutdown_incarnations) =
        join_targets(&[child], &["shutdown-incarnation"]);
    let shutdown_obligation = input_queue
        .register_join_obligation(parent, "shutdown-turn".to_string(), shutdown_targets)
        .await
        .expect("replacement registration should succeed after a stale edge");
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                shutdown_obligation.generation(),
                child,
                &shutdown_incarnations[0],
                "shutdown-incarnation",
                AgentStatus::Shutdown,
            )
            .await
            .is_some()
    );
    assert!(
        input_queue
            .consume_ready_join_obligation("shutdown-turn", shutdown_obligation.generation(),)
            .await
    );
    assert!(
        !input_queue
            .consume_ready_join_obligation("shutdown-turn", 999)
            .await
    );
    let (new_targets, new_incarnations) = join_targets(&[child], &["new-incarnation"]);
    let replacement_obligation = input_queue
        .register_join_obligation(parent, "replacement-turn-2".to_string(), new_targets)
        .await
        .expect("replacement registration should succeed");
    assert!(
        input_queue
            .resolve_join_target(
                parent,
                replacement_obligation.generation(),
                child,
                &new_incarnations[0],
                "new-incarnation",
                AgentStatus::Completed(None),
            )
            .await
            .is_some()
    );
    assert!(
        input_queue
            .consume_ready_join_obligation(
                "replacement-turn-2",
                replacement_obligation.generation(),
            )
            .await
    );
}
