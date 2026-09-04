use super::*;
use crate::state::TurnState;
use pretty_assertions::assert_eq;

struct NeverEndingCapacityTask;

impl SessionTask for NeverEndingCapacityTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "agent_control_tests.capacity_ready"
    }

    async fn run(
        self: Arc<Self>,
        _session: Arc<crate::session::session::Session>,
        _ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        cancellation_token.cancelled().await;
        Ok(None)
    }
}

struct CompletesOnNotifyTask {
    release: Arc<tokio::sync::Notify>,
}

impl SessionTask for CompletesOnNotifyTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "agent_control_tests.completes_on_notify"
    }

    async fn run(
        self: Arc<Self>,
        _session: Arc<crate::session::session::Session>,
        _ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        _cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        self.release.notified().await;
        Ok(Some("completed after release".to_string()))
    }
}

async fn b6_gated_turn(
    thread: &Arc<CodexThread>,
    turn_id: &str,
) -> (
    Arc<TurnContext>,
    Arc<tokio::sync::Mutex<TurnState>>,
    Arc<tokio::sync::Notify>,
) {
    let turn = thread
        .session
        .new_turn_with_default_settings(turn_id.to_string(), Default::default())
        .await;
    let release = Arc::new(tokio::sync::Notify::new());
    thread
        .session
        .start_task(
            Arc::clone(&turn),
            Vec::new(),
            CompletesOnNotifyTask {
                release: Arc::clone(&release),
            },
        )
        .await;
    let state = thread
        .session
        .active_turn
        .lock()
        .await
        .as_ref()
        .map(|active| Arc::clone(&active.turn_state))
        .expect("gated turn should remain active");
    (turn, state, release)
}

async fn b6_spawn_pathful_child(
    harness: &AgentControlHarness,
    parent_thread_id: ThreadId,
    parent_thread: &Arc<CodexThread>,
    parent_turn_id: &str,
    name: &str,
) -> (ThreadId, Arc<CodexThread>) {
    let source = thread_spawn_source(
        parent_thread_id,
        &parent_thread.session_source,
        next_thread_spawn_depth(&parent_thread.session_source),
        /*agent_role*/ None,
        Some(name.to_string()),
    )
    .expect("pathful child source");
    let thread_id = harness
        .control
        .spawn_agent_with_metadata(
            harness.config.clone(),
            text_input(name),
            Some(source),
            SpawnAgentOptions {
                parent_thread_id: Some(parent_thread_id),
                parent_turn_id: Some(parent_turn_id.to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("pathful child should start")
        .thread_id;
    let thread = harness
        .manager
        .get_thread(thread_id)
        .await
        .expect("pathful child should remain addressable");
    let expected_path =
        AgentPath::try_from(format!("/root/{name}").as_str()).expect("expected pathful child path");
    assert_eq!(
        harness
            .control
            .get_agent_metadata(thread_id)
            .and_then(|metadata| metadata.agent_path),
        Some(expected_path)
    );
    (thread_id, thread)
}

async fn b6_seed_join(
    parent: &Arc<CodexThread>,
    parent_turn_id: &str,
    target_thread_id: ThreadId,
    target_identity: &Arc<dyn std::any::Any + Send + Sync>,
    target_turn_id: &str,
    retain: bool,
) -> u64 {
    let token = parent
        .session
        .input_queue
        .register_join_obligation(
            parent.session.thread_id,
            parent_turn_id.to_string(),
            std::collections::HashMap::from([(
                target_thread_id,
                (Arc::clone(target_identity), target_turn_id.to_string()),
            )]),
        )
        .await
        .expect("join should register");
    let status = AgentStatus::Completed(Some(format!("{target_turn_id} terminal")));
    let ready = if retain {
        parent
            .session
            .input_queue
            .resolve_join_target_retaining_successor(
                parent.session.thread_id,
                token.generation(),
                target_thread_id,
                target_identity,
                target_turn_id,
                status,
            )
            .await
            .and_then(|resolution| resolution.completion)
    } else {
        parent
            .session
            .input_queue
            .resolve_join_target(
                parent.session.thread_id,
                token.generation(),
                target_thread_id,
                target_identity,
                target_turn_id,
                status,
            )
            .await
    };
    assert!(ready.is_some(), "single-target join should become ready");
    token.generation()
}

async fn b6_enqueue_join(parent: &Arc<CodexThread>, parent_turn_id: &str, generation: u64) {
    parent
        .session
        .input_queue
        .mark_join_trigger(parent_turn_id, generation)
        .await;
    let mut options = TurnStartOptions::default();
    options.parent_turn_id = Some(parent_turn_id.to_string());
    parent
        .session
        .input_queue
        .enqueue_mailbox_communication(
            InterAgentCommunication::new(
                AgentPath::try_from("/root/child").expect("child path"),
                AgentPath::root(),
                Vec::new(),
                format!("join {parent_turn_id}"),
                true,
            ),
            options,
        )
        .await;
}

async fn b6_wait_status(thread: &Arc<CodexThread>, expected: AgentStatus) {
    let mut rx = thread.subscribe_status();
    timeout(Duration::from_secs(2), async {
        while rx.borrow().clone() != expected {
            rx.changed()
                .await
                .expect("status stream should remain live");
        }
    })
    .await
    .expect("expected terminal status should become observable");
}

enum B6RootTermination {
    Completed,
    Aborted(TurnAbortReason),
}

async fn b6_assert_root_releases_exact_join_continuation(termination: B6RootTermination) {
    let harness = AgentControlHarness::new().await;
    let (root_id, root) = harness.start_thread().await;
    let child_id = ThreadId::new();
    let child: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
    let generation = b6_seed_join(&root, "A0", child_id, &child, "leaf", false).await;
    b6_enqueue_join(&root, "A0", generation).await;
    let (_, state, release) = b6_gated_turn(&root, "A1").await;
    let Some((observed_generation, Some((successor_turn_id, leased_state, terminal_status)))) =
        root.session
            .input_queue
            .join_continuation_for_predecessor(root_id, "A0")
            .await
            .expect("A0 continuation lookup")
    else {
        panic!("real A1 startup should install the exact A0 continuation lease");
    };
    assert_eq!(observed_generation, generation);
    assert_eq!(successor_turn_id, "A1");
    assert!(Arc::ptr_eq(&leased_state, &state));
    assert_eq!(terminal_status, None);
    drop(leased_state);

    match termination {
        B6RootTermination::Completed => {
            release.notify_one();
            b6_wait_status(
                &root,
                AgentStatus::Completed(Some("completed after release".into())),
            )
            .await;
            timeout(Duration::from_secs(2), async {
                loop {
                    let idle = root.session.active_turn.lock().await.is_none();
                    let lease_released = root
                        .session
                        .input_queue
                        .join_continuation_for_predecessor(root_id, "A0")
                        .await
                        .expect("A0 continuation lookup should stay unambiguous")
                        .is_none();
                    if idle && lease_released && Arc::strong_count(&state) == 1 {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("completed root hook should release its exact continuation lease and Arc");
        }
        B6RootTermination::Aborted(reason) => {
            root.session.abort_all_tasks(reason).await;
            assert!(root.session.active_turn.lock().await.is_none());
            assert!(
                root.session
                    .input_queue
                    .join_continuation_for_predecessor(root_id, "A0")
                    .await
                    .expect("A0 continuation lookup after root abort")
                    .is_none(),
                "root abort hook should release the exact A0 to A1 continuation lease"
            );
            assert_eq!(Arc::strong_count(&state), 1);
        }
    }
}

struct B5CapacityFixture {
    harness: AgentControlHarness,
    parent_thread_id: ThreadId,
    parent_thread: Arc<CodexThread>,
    target_thread_id: ThreadId,
    target_thread: Arc<CodexThread>,
}

async fn b5_capacity_fixture() -> B5CapacityFixture {
    b5_capacity_fixture_with_limit(2).await
}

async fn b5_capacity_fixture_with_limit(max_threads: i64) -> B5CapacityFixture {
    b5_capacity_fixture_with_limit_and_generator(max_threads, ThreadId::new).await
}

async fn b5_capacity_fixture_with_limit_and_generator(
    max_threads: i64,
    generator: impl Fn() -> ThreadId + Send + Sync + 'static,
) -> B5CapacityFixture {
    let (home, mut config) = test_config_with_cli_overrides(vec![(
        "agents.max_threads".to_string(),
        TomlValue::Integer(max_threads),
    )])
    .await;
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("test config should allow feature update");
    let harness = AgentControlHarness::new_with_config_and_thread_id_generator(
        home,
        config.clone(),
        generator,
    )
    .await;
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

    B5CapacityFixture {
        harness,
        parent_thread_id,
        parent_thread,
        target_thread_id,
        target_thread,
    }
}

async fn shutdown_terminal_while_preserving_saturation(
    harness: &AgentControlHarness,
    target_thread: &Arc<CodexThread>,
) -> AgentExecutionReservation {
    let mut release_rx = harness.control.agent_execution_limiter.subscribe_release();
    target_thread
        .submit(Op::Shutdown {})
        .await
        .expect("terminal target shutdown should submit");
    timeout(Duration::from_secs(5), async {
        loop {
            release_rx
                .changed()
                .await
                .expect("execution limiter should remain available");
            if let Ok(reservation) = harness.control.reserve_execution_capacity(
                MultiAgentVersion::V2,
                &SessionSource::SubAgent(SubAgentSource::Other(
                    "terminal-capacity-replacement".to_string(),
                )),
            ) {
                break reservation;
            }
        }
    })
    .await
    .expect("terminal target capacity should be replaced before shutdown completes")
}

#[path = "owned_join_tests/capacity_tests.rs"]
mod capacity_tests;
#[path = "owned_join_tests/recursive_delivery_tests.rs"]
mod recursive_delivery_tests;
