use crate::agent::AgentControl;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::Ordering;

fn control_with_limit(max_threads: usize) -> AgentControl {
    let control = AgentControl::default();
    control.agent_execution_limiter.initialize(max_threads);
    control
}

#[test]
fn execution_guards_count_active_v2_subagent_turns() {
    let control = control_with_limit(/*max_threads*/ 1);
    // Child role configs cannot replace the root-derived session limit.
    control
        .agent_execution_limiter
        .initialize(/*max_threads*/ 2);
    let source = SessionSource::SubAgent(SubAgentSource::Other("worker".to_string()));

    let first = control
        .reserve_execution_capacity(MultiAgentVersion::V2, &source)
        .expect("first active turn should reserve capacity");
    let Err(err) = control.ensure_execution_capacity(MultiAgentVersion::V2, &source) else {
        panic!("second active turn should exceed the derived non-root cap");
    };
    let CodexErrorDetails::AgentLimitReached { max_threads } = err.details() else {
        panic!("expected AgentLimitReached");
    };
    assert_eq!(*max_threads, 1);

    drop(first);
    let released = control
        .reserve_execution_capacity(MultiAgentVersion::V2, &source)
        .expect("capacity should be released when the running task drops");
    drop(released);
}

#[test]
fn execution_guards_ignore_root_and_v1_turns() {
    let control = control_with_limit(/*max_threads*/ 0);

    let root = control
        .reserve_execution_capacity(MultiAgentVersion::V2, &SessionSource::Cli)
        .expect("root execution should remain unlimited");
    let v1 = control
        .reserve_execution_capacity(
            MultiAgentVersion::V1,
            &SessionSource::SubAgent(SubAgentSource::Other("worker".to_string())),
        )
        .expect("V1 execution should remain unlimited");
    assert_eq!(
        control
            .agent_execution_limiter
            .active
            .load(Ordering::Acquire),
        0
    );
    drop((root, v1));
}

#[test]
fn execution_capacity_reservation_is_atomic_at_effective_v2_child_boundary() {
    let control = control_with_limit(/*max_threads*/ 127);
    let source = SessionSource::SubAgent(SubAgentSource::Other("worker".to_string()));
    let held_guards = (0..126)
        .map(|_| {
            control
                .reserve_execution_capacity(MultiAgentVersion::V2, &source)
                .expect("held V2 subagent execution should reserve capacity")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        control
            .agent_execution_limiter
            .active
            .load(Ordering::Acquire),
        126
    );

    let barrier = Arc::new(Barrier::new(/*n*/ 3));
    let reservations = std::thread::scope(|scope| {
        let control = &control;
        let source = &source;
        let first_barrier = Arc::clone(&barrier);
        let first = scope.spawn(move || {
            first_barrier.wait();
            control.reserve_execution_capacity(MultiAgentVersion::V2, source)
        });
        let second_barrier = Arc::clone(&barrier);
        let second = scope.spawn(move || {
            second_barrier.wait();
            control.reserve_execution_capacity(MultiAgentVersion::V2, source)
        });
        barrier.wait();
        [
            first.join().expect("first contender should join"),
            second.join().expect("second contender should join"),
        ]
    });

    let admitted = reservations.iter().filter(|result| result.is_ok()).count();
    let refused = reservations.iter().filter(|result| result.is_err()).count();
    let active_after_race = control
        .agent_execution_limiter
        .active
        .load(Ordering::Acquire);
    assert_eq!(
        (admitted, refused, active_after_race),
        (1, 1, 127),
        "the final effective child slot must be reserved atomically"
    );

    let [first, second] = reservations;
    let (winner, error) = match (first, second) {
        (Ok(winner), Err(error)) | (Err(error), Ok(winner)) => (winner, error),
        _ => panic!("exactly one contender should own the final slot"),
    };
    let CodexErrorDetails::AgentLimitReached { max_threads } = error.details() else {
        panic!("expected AgentLimitReached from the losing reservation");
    };
    assert_eq!(*max_threads, 127);
    let Err(err) = control.ensure_execution_capacity(MultiAgentVersion::V2, &source) else {
        panic!("the held winner should keep the effective child boundary full");
    };
    let CodexErrorDetails::AgentLimitReached { max_threads } = err.details() else {
        panic!("expected AgentLimitReached while the winner is held");
    };
    assert_eq!(*max_threads, 127);

    drop(winner);
    assert_eq!(
        control
            .agent_execution_limiter
            .active
            .load(Ordering::Acquire),
        126
    );
    let reacquired = control
        .reserve_execution_capacity(MultiAgentVersion::V2, &source)
        .expect("the released slot should be reacquired exactly once");
    assert_eq!(
        control
            .agent_execution_limiter
            .active
            .load(Ordering::Acquire),
        127
    );

    drop(reacquired);
    assert_eq!(
        control
            .agent_execution_limiter
            .active
            .load(Ordering::Acquire),
        126
    );
    drop(held_guards);
    assert_eq!(
        control
            .agent_execution_limiter
            .active
            .load(Ordering::Acquire),
        0
    );
}

#[tokio::test]
async fn agent_execution_limiter_release_notifies_capacity_waiter() {
    let control = control_with_limit(/*max_threads*/ 1);
    let source = SessionSource::SubAgent(SubAgentSource::Other("worker".to_string()));
    let held = control
        .reserve_execution_capacity(MultiAgentVersion::V2, &source)
        .expect("the only child slot should be held");
    let mut release_rx = control.agent_execution_limiter.subscribe_release();

    drop(held);

    release_rx
        .changed()
        .await
        .expect("a guard drop must publish capacity eligibility");
    assert_eq!(*release_rx.borrow(), 1);
}
