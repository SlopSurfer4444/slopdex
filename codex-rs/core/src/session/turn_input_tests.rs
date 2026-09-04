use super::*;
use crate::agent::AgentControl;
use crate::config::Constrained;
use crate::session::step_settings::StepSettingsUpdate;
use crate::session::tests::make_session_and_context;
use crate::session::tests::make_session_and_context_with_rx;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::PendingWakeClaimBarrier;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use codex_protocol::AgentPath;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::ServiceTier;
use codex_protocol::config_types::Settings;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::turn_input::TurnInput as SubmittedTurnInput;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::test_codex::local_selections;
use pretty_assertions::assert_eq;
use test_case::test_case;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
struct NeverEndingTask {
    kind: TaskKind,
    listen_to_cancellation_token: bool,
}

impl SessionTask for NeverEndingTask {
    fn kind(&self) -> TaskKind {
        self.kind
    }

    fn span_name(&self) -> &'static str {
        "session_task.turn_input_test"
    }

    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        if self.listen_to_cancellation_token {
            cancellation_token.cancelled().await;
            return Ok(None);
        }
        loop {
            sleep(std::time::Duration::from_secs(60)).await;
        }
    }
}

fn user_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

async fn submit_start_only(
    session: &Arc<Session>,
    input: SubmittedTurnInput,
) -> TurnInputSubmission {
    handle(
        session,
        TurnInputRequest::new(input),
        TurnInputMode::StartIfIdle,
        "test-submission".to_string(),
    )
    .await
    .expect("start-only submission should be valid")
}

async fn submit_steer_only(
    session: &Arc<Session>,
    input: Vec<UserInput>,
    expected_turn_id: &str,
) -> TurnInputSubmission {
    handle(
        session,
        TurnInputRequest::new(SubmittedTurnInput::UserInput {
            content: input,
            client_id: None,
        }),
        TurnInputMode::Steer {
            expected_turn_id: expected_turn_id.to_string(),
        },
        "test-submission".to_string(),
    )
    .await
    .expect("steer-only submission should be valid")
}

#[tokio::test]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "simulate an in-flight realtime append while checking input admission"
)]
async fn steering_does_not_wait_for_realtime_history() {
    let (mut session, turn_context) = make_session_and_context().await;
    session.realtime_history = Some(tokio::sync::Mutex::new(Default::default()));
    let session = Arc::new(session);
    let turn_context = Arc::new(turn_context);
    session
        .spawn_task(
            Arc::clone(&turn_context),
            Vec::new(),
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: true,
            },
        )
        .await;

    let history = session
        .realtime_history
        .as_ref()
        .expect("realtime history")
        .lock()
        .await;
    for mode in [
        TurnInputMode::StartOrSteer,
        TurnInputMode::Steer {
            expected_turn_id: turn_context.sub_id.clone(),
        },
    ] {
        let submission = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            handle(
                &session,
                TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "steer without waiting for persistence".to_string(),
                    text_elements: Vec::new(),
                }]),
                mode,
                "steer-submission".to_string(),
            ),
        )
        .await
        .expect("steering must not wait for the realtime recorder")
        .expect("steering should succeed");
        assert_eq!(
            submission,
            TurnInputSubmission::Steered {
                turn_id: turn_context.sub_id.clone()
            }
        );
    }
    drop(history);
    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
}

#[tokio::test]
async fn accepted_input_applies_thread_settings() {
    let (session, turn_context, _rx) = make_session_and_context_with_rx().await;
    let config = session.get_config().await;
    handle(
        &session,
        TurnInputRequest::user_input(vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }])
        .with_thread_settings(ThreadSettingsOverrides {
            environments: Some(local_selections(config.cwd.clone())),
            approval_policy: Some(config.permissions.approval_policy.value()),
            approvals_reviewer: Some(codex_config::types::ApprovalsReviewer::AutoReview),
            sandbox_policy: Some(config.legacy_sandbox_policy()),
            summary: config.model_reasoning_summary,
            personality: config.personality,
            collaboration_mode: Some(CollaborationMode {
                mode: ModeKind::Default,
                settings: Settings {
                    model: turn_context.model_info().slug.clone(),
                    reasoning_effort: config.model_reasoning_effort.clone(),
                    developer_instructions: None,
                },
            }),
            ..Default::default()
        }),
        TurnInputMode::StartOrSteer,
        "sub-1".to_string(),
    )
    .await
    .expect("submit user turn");

    let state = session.state.lock().await;
    assert_eq!(
        state.session_configuration.step_settings.approvals_reviewer,
        codex_config::types::ApprovalsReviewer::AutoReview
    );
    assert!(
        session.mcp_refresh.is_pending(),
        "server elicitation authority changes must refresh MCP state"
    );
}

#[tokio::test]
async fn saturated_v2_direct_start_and_recovery_have_no_side_effects() {
    let (mut session, _turn_context) = make_session_and_context().await;
    let session_source = SessionSource::SubAgent(SubAgentSource::Other("worker".to_string()));
    session
        .state
        .lock()
        .await
        .session_configuration
        .session_source = session_source.clone();
    session.multi_agent_version = std::sync::OnceLock::from(MultiAgentVersion::V2);
    session.services.agent_control =
        AgentControl::default().with_session_id(Default::default(), /*max_threads*/ 1);
    let held = session
        .services
        .agent_control
        .reserve_execution_capacity(MultiAgentVersion::V2, &session_source)
        .expect("fixture should hold the only V2 execution permit");
    let session = Arc::new(session);
    let settings_before = session.thread_settings_snapshot().await;

    let direct_error = handle(
        &session,
        TurnInputRequest::user_input(vec![UserInput::Text {
            text: "must not start".to_string(),
            text_elements: Vec::new(),
        }])
        .with_thread_settings(ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            ..Default::default()
        }),
        TurnInputMode::StartOrSteer,
        "direct-saturated".to_string(),
    )
    .await
    .expect_err("saturated direct start must fail before committing settings");
    assert!(matches!(
        direct_error.details(),
        CodexErrorDetails::AgentLimitReached { max_threads: 1 }
    ));
    assert_eq!(session.thread_settings_snapshot().await, settings_before);
    assert!(session.active_turn.lock().await.is_none());

    let recovery_error = handle_recovery(
        &session,
        ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            ..Default::default()
        },
        TurnStartOptions::default(),
        "recovery-saturated".to_string(),
    )
    .await
    .expect_err("saturated recovery must fail before committing settings");
    assert!(matches!(
        recovery_error.details(),
        CodexErrorDetails::AgentLimitReached { max_threads: 1 }
    ));
    assert_eq!(session.thread_settings_snapshot().await, settings_before);
    assert!(session.active_turn.lock().await.is_none());
    drop(held);
}

#[tokio::test]
async fn user_start_preempts_uncommitted_pending_wake_reservation() {
    let (mut session, _turn_context) = make_session_and_context().await;
    let session_source = SessionSource::SubAgent(SubAgentSource::Other("worker".to_string()));
    session
        .state
        .lock()
        .await
        .session_configuration
        .session_source = session_source.clone();
    session.multi_agent_version = std::sync::OnceLock::from(MultiAgentVersion::V2);
    session.services.agent_control =
        AgentControl::default().with_session_id(Default::default(), /*max_threads*/ 1);
    let session = Arc::new(session);
    let pending_mail = InterAgentCommunication::new(
        AgentPath::root(),
        AgentPath::root(),
        Vec::new(),
        "pending trigger".to_string(),
        /*trigger_turn*/ true,
    );
    session
        .input_queue
        .enqueue_mailbox_communication(pending_mail.clone(), Default::default())
        .await;

    let barrier = PendingWakeClaimBarrier::new();
    let pending_session = Arc::clone(&session);
    let pending_barrier = barrier.clone();
    let pending_wake = tokio::spawn(async move {
        pending_session
            .maybe_start_turn_for_pending_work_with_sub_id_and_barrier(
                "pending-wake".to_string(),
                pending_barrier,
            )
            .await;
    });
    barrier.wait_until_claimed().await;

    let submission = start_or_steer_with_task(
        &session,
        TurnInputRequest::user_input(vec![UserInput::Text {
            text: "priority user start".to_string(),
            text_elements: Vec::new(),
        }]),
        "priority-user".to_string(),
        NeverEndingTask {
            kind: TaskKind::Regular,
            listen_to_cancellation_token: true,
        },
    )
    .await
    .expect("the user start must inherit the pending wake's final permit");
    assert_eq!(
        submission,
        TurnInputSubmission::Started {
            turn_id: "priority-user".to_string(),
        }
    );

    barrier.release();
    pending_wake
        .await
        .expect("pending wake should unwind cleanly");

    let turn_state = {
        let active_turn = session.active_turn.lock().await;
        let active_turn = active_turn.as_ref().expect("user turn should be active");
        assert!(active_turn.task.is_some(), "user task should be running");
        Arc::clone(&active_turn.turn_state)
    };
    let saturated = match session
        .services
        .agent_control
        .reserve_execution_capacity(MultiAgentVersion::V2, &session_source)
    {
        Ok(_) => panic!("exactly one running task must own the only permit"),
        Err(error) => error,
    };
    assert!(matches!(
        saturated.details(),
        CodexErrorDetails::AgentLimitReached { max_threads: 1 }
    ));
    let pending_input = session
        .input_queue
        .take_pending_input_for_turn_state(turn_state.as_ref())
        .await;
    assert_eq!(pending_input.len(), 1);
    assert!(matches!(
        &pending_input[0],
        TurnInput::InterAgentCommunication(mail) if mail == &pending_mail
    ));
    assert!(!session.input_queue.has_pending_mailbox_items().await);

    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    let released = session
        .services
        .agent_control
        .reserve_execution_capacity(MultiAgentVersion::V2, &session_source)
        .expect("aborting the user task must release the inherited permit");
    drop(released);
}

#[tokio::test]
async fn cancelled_committed_pending_wake_releases_claim_for_user_start() {
    let (mut session, _turn_context) = make_session_and_context().await;
    let session_source = SessionSource::SubAgent(SubAgentSource::Other("worker".to_string()));
    session
        .state
        .lock()
        .await
        .session_configuration
        .session_source = session_source.clone();
    session.multi_agent_version = std::sync::OnceLock::from(MultiAgentVersion::V2);
    session.services.agent_control =
        AgentControl::default().with_session_id(Default::default(), /*max_threads*/ 1);
    let session = Arc::new(session);
    let pending_mail = InterAgentCommunication::new(
        AgentPath::root(),
        AgentPath::root(),
        Vec::new(),
        "pending cancellation trigger".to_string(),
        /*trigger_turn*/ true,
    );
    session
        .input_queue
        .enqueue_mailbox_communication(pending_mail.clone(), Default::default())
        .await;

    let barrier = PendingWakeClaimBarrier::after_commit();
    let pending_session = Arc::clone(&session);
    let pending_barrier = barrier.clone();
    let pending_wake = tokio::spawn(async move {
        pending_session
            .maybe_start_turn_for_pending_work_with_sub_id_and_barrier(
                "cancelled-pending-wake".to_string(),
                pending_barrier,
            )
            .await;
    });
    barrier.wait_until_claimed().await;
    pending_wake.abort();
    assert!(
        pending_wake
            .await
            .expect_err("the committed pending wake should be cancelled")
            .is_cancelled()
    );
    assert!(session.input_queue.has_trigger_turn_mailbox_items().await);

    let submission = start_or_steer_with_task(
        &session,
        TurnInputRequest::user_input(vec![UserInput::Text {
            text: "user after pending cancellation".to_string(),
            text_elements: Vec::new(),
        }]),
        "user-after-pending-cancellation".to_string(),
        NeverEndingTask {
            kind: TaskKind::Regular,
            listen_to_cancellation_token: true,
        },
    )
    .await
    .expect("cancelled pending wake must not strand the final permit");
    assert!(matches!(submission, TurnInputSubmission::Started { .. }));

    let turn_state = {
        let active_turn = session.active_turn.lock().await;
        let active_turn = active_turn.as_ref().expect("user turn should be active");
        assert!(active_turn.task.is_some());
        Arc::clone(&active_turn.turn_state)
    };
    let pending_input = session
        .input_queue
        .take_pending_input_for_turn_state(turn_state.as_ref())
        .await;
    assert_eq!(pending_input.len(), 1);
    assert!(matches!(
        &pending_input[0],
        TurnInput::InterAgentCommunication(mail) if mail == &pending_mail
    ));
    assert!(!session.input_queue.has_pending_mailbox_items().await);

    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    let released = session
        .services
        .agent_control
        .reserve_execution_capacity(MultiAgentVersion::V2, &session_source)
        .expect("cancelled pending wake and aborted user task must release the permit");
    drop(released);
}

#[tokio::test]
async fn cancelled_explicit_user_preparation_releases_taskless_claim() {
    let (mut session, _turn_context) = make_session_and_context().await;
    let session_source = SessionSource::SubAgent(SubAgentSource::Other("worker".to_string()));
    session
        .state
        .lock()
        .await
        .session_configuration
        .session_source = session_source.clone();
    session.multi_agent_version = std::sync::OnceLock::from(MultiAgentVersion::V2);
    session.services.agent_control =
        AgentControl::default().with_session_id(Default::default(), /*max_threads*/ 1);
    let session = Arc::new(session);

    let barrier = UserStartClaimBarrier::new();
    let cancelled_session = Arc::clone(&session);
    let cancelled_barrier = barrier.clone();
    let cancelled_start = tokio::spawn(async move {
        start_or_steer_with_task_and_barrier(
            &cancelled_session,
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "cancel during preparation".to_string(),
                text_elements: Vec::new(),
            }]),
            "cancelled-user-preparation".to_string(),
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: true,
            },
            cancelled_barrier,
        )
        .await
    });
    barrier.wait_until_claimed().await;
    cancelled_start.abort();
    assert!(
        cancelled_start
            .await
            .expect_err("the explicit user preparation should be cancelled")
            .is_cancelled()
    );

    let submission = start_or_steer_with_task(
        &session,
        TurnInputRequest::user_input(vec![UserInput::Text {
            text: "user after preparation cancellation".to_string(),
            text_elements: Vec::new(),
        }]),
        "user-after-preparation-cancellation".to_string(),
        NeverEndingTask {
            kind: TaskKind::Regular,
            listen_to_cancellation_token: true,
        },
    )
    .await
    .expect("cancelled preparation must not strand the final permit");
    assert!(matches!(submission, TurnInputSubmission::Started { .. }));
    assert!(
        session
            .active_turn
            .lock()
            .await
            .as_ref()
            .is_some_and(|turn| turn.task.is_some())
    );

    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    let released = session
        .services
        .agent_control
        .reserve_execution_capacity(MultiAgentVersion::V2, &session_source)
        .expect("cancelled preparation and aborted user task must release the permit");
    drop(released);
}

#[tokio::test]
async fn start_only_rejects_active_turn_without_injecting() {
    let (session, turn_context, _rx) = make_session_and_context_with_rx().await;
    session
        .spawn_task(
            Arc::clone(&turn_context),
            Vec::new(),
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: true,
            },
        )
        .await;

    let input = SubmittedTurnInput::ResponseItem(user_message("synthetic idle input"));
    let submission = submit_start_only(&session, input).await;

    assert_eq!(
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::NotIdle,
        },
        submission
    );
    assert_eq!(
        Vec::<TurnInput>::new(),
        session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await
            .0
    );

    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
}

#[tokio::test]
async fn recovery_rejects_active_turn_without_injecting_or_applying_settings() {
    let (session, turn_context, _rx) = make_session_and_context_with_rx().await;
    let original_approval_policy = session
        .get_config()
        .await
        .permissions
        .approval_policy
        .value();
    session
        .spawn_task(
            Arc::clone(&turn_context),
            Vec::new(),
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: true,
            },
        )
        .await;

    let submission = handle_recovery(
        &session,
        ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            ..Default::default()
        },
        TurnStartOptions::default(),
        "recovered-turn".to_string(),
    )
    .await
    .expect("recovery should return a typed rejection");

    assert_eq!(
        submission,
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::NotIdle,
        }
    );
    assert_eq!(
        session
            .get_config()
            .await
            .permissions
            .approval_policy
            .value(),
        original_approval_policy
    );
    assert_eq!(
        session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await
            .0,
        Vec::<TurnInput>::new()
    );

    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
}

#[tokio::test]
async fn start_only_rejects_current_plan_before_validating_settings() {
    let (session, _turn_context, _rx) = make_session_and_context_with_rx().await;
    let default_mode = session.collaboration_mode().await;
    {
        let mut state = session.state.lock().await;
        let settings = Arc::make_mut(&mut state.session_configuration.step_settings);
        settings.collaboration_mode.mode = ModeKind::Plan;
        settings.approval_policy = Constrained::allow_only(AskForApproval::OnRequest);
    }
    let desired_settings = session.thread_settings_snapshot().await;
    let invalid_override = ThreadSettingsOverrides {
        collaboration_mode: Some(default_mode.clone()),
        approval_policy: Some(AskForApproval::Never),
        ..Default::default()
    };

    // Current Plan takes precedence even when the request would leave Plan or
    // fail settings validation. Nothing has been reserved or applied yet.
    let submission = handle(
        &session,
        TurnInputRequest::new(SubmittedTurnInput::ResponseItem(user_message(
            "synthetic idle input",
        )))
        .with_thread_settings(invalid_override.clone()),
        TurnInputMode::StartIfIdle,
        "automatic-plan-submission".to_string(),
    )
    .await
    .expect("current Plan must reject before settings validation");
    assert_eq!(
        submission,
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::PlanMode,
        }
    );
    assert_eq!(session.thread_settings_snapshot().await, desired_settings);
    assert!(session.active_turn.lock().await.is_none());

    session
        .update_settings(SessionSettingsUpdate {
            step_settings: StepSettingsUpdate {
                collaboration_mode: Some(default_mode),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("explicit settings may leave Plan mode");
    let desired_settings = session.thread_settings_snapshot().await;
    let result = handle(
        &session,
        TurnInputRequest::new(SubmittedTurnInput::ResponseItem(user_message(
            "invalid automatic input",
        )))
        .with_thread_settings(invalid_override),
        TurnInputMode::StartIfIdle,
        "invalid-automatic-submission".to_string(),
    )
    .await;
    let error = result.expect_err("invalid automatic settings must be rejected");
    assert!(matches!(
        error.details(),
        CodexErrorDetails::InvalidRequest(_)
    ));
    assert_eq!(session.thread_settings_snapshot().await, desired_settings);
    assert!(session.active_turn.lock().await.is_none());
    assert_eq!(
        Vec::<TurnInput>::new(),
        session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await
            .0
    );
}

#[tokio::test]
async fn prepared_user_updates_merge_with_settings_at_turn_start() {
    for (requested, intervening) in [
        (
            ThreadSettingsOverrides {
                effort: Some(Some(ReasoningEffort::High)),
                ..Default::default()
            },
            ThreadSettingsOverrides {
                model: Some("gpt-5.2".to_string()),
                ..Default::default()
            },
        ),
        (
            ThreadSettingsOverrides {
                model: Some("gpt-5.2".to_string()),
                ..Default::default()
            },
            ThreadSettingsOverrides {
                effort: Some(Some(ReasoningEffort::High)),
                ..Default::default()
            },
        ),
    ] {
        let (session, _turn_context, _rx) = make_session_and_context_with_rx().await;
        let initial = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "gpt-5.4".to_string(),
                reasoning_effort: Some(ReasoningEffort::Low),
                developer_instructions: None,
            },
        };
        session
            .update_settings(thread_settings::prepare_update(ThreadSettingsOverrides {
                collaboration_mode: Some(initial.clone()),
                ..Default::default()
            }))
            .await
            .expect("set initial model and effort");
        let prepared =
            PreparedTurnInputSettings::prepare(&session, requested, TurnStartOptions::default())
                .await
                .expect("prepare partial protocol update");
        session
            .update_settings(thread_settings::prepare_update(intervening))
            .await
            .expect("commit intervening settings");

        let turn_context = prepared
            .apply_started(
                &session,
                "sparse-user-start".to_string(),
                TurnStartKind::User,
            )
            .await
            .expect("apply prepared settings")
            .expect("user start is permitted");
        let expected = initial.with_updates(
            Some("gpt-5.2".to_string()),
            Some(Some(ReasoningEffort::High)),
            /*developer_instructions*/ None,
        );
        assert_eq!(session.collaboration_mode().await, expected);
        assert_eq!(turn_context.collaboration_mode(), expected);
        assert_eq!(
            turn_context.initial_settings.selected_collaboration_mode(),
            &expected
        );
        assert!(Arc::ptr_eq(
            &turn_context.initial_settings.model_info,
            turn_context.model_info(),
        ));
    }
}

#[tokio::test]
async fn automatic_admission_uses_current_candidate_after_plan_preview() {
    let (session, _turn_context, _rx) = make_session_and_context_with_rx().await;
    let default_mode = session.collaboration_mode().await;
    let mut plan_mode = default_mode.clone();
    plan_mode.mode = ModeKind::Plan;
    session
        .update_settings(thread_settings::prepare_update(ThreadSettingsOverrides {
            collaboration_mode: Some(plan_mode),
            ..Default::default()
        }))
        .await
        .expect("enter Plan after the initial admission check");
    let prepared = PreparedTurnInputSettings::prepare(
        &session,
        ThreadSettingsOverrides {
            effort: Some(Some(ReasoningEffort::High)),
            ..Default::default()
        },
        TurnStartOptions::default(),
    )
    .await
    .expect("validate the patch while the preview is Plan");
    session
        .update_settings(thread_settings::prepare_update(ThreadSettingsOverrides {
            collaboration_mode: Some(default_mode.clone()),
            ..Default::default()
        }))
        .await
        .expect("leave Plan before atomic admission");

    let turn_context = prepared
        .apply_started(
            &session,
            "automatic-after-plan-preview".to_string(),
            TurnStartKind::Automatic,
        )
        .await
        .expect("automatic admission should succeed")
        .expect("current and proposed modes are both Default");
    let expected = default_mode.with_updates(
        /*model*/ None,
        Some(Some(ReasoningEffort::High)),
        /*developer_instructions*/ None,
    );
    assert_eq!(session.collaboration_mode().await, expected);
    assert_eq!(turn_context.collaboration_mode(), expected);
    assert_eq!(
        turn_context.initial_settings.selected_collaboration_mode(),
        &expected
    );
}

#[tokio::test]
async fn automatic_admission_rechecks_plan_mode_without_committing_sparse_settings() {
    struct ConfigRecorder(Arc<std::sync::Mutex<Vec<(AskForApproval, ApprovalsReviewer)>>>);

    impl codex_extension_api::ConfigContributor<crate::config::Config> for ConfigRecorder {
        fn on_config_changed(
            &self,
            _session_store: &codex_extension_api::ExtensionData,
            _thread_store: &codex_extension_api::ExtensionData,
            _previous_config: &crate::config::Config,
            new_config: &crate::config::Config,
        ) {
            self.0.lock().expect("config records lock").push((
                new_config.permissions.approval_policy.value(),
                new_config.approvals_reviewer,
            ));
        }
    }

    let (mut session, _turn_context, rx) = make_session_and_context_with_rx().await;
    let records = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut extensions =
        codex_extension_api::ExtensionRegistryBuilder::<crate::config::Config>::new();
    extensions.config_contributor(Arc::new(ConfigRecorder(Arc::clone(&records))));
    Arc::get_mut(&mut session)
        .expect("unique test session")
        .services
        .extensions = Arc::new(extensions.build());
    {
        let mut state = session.state.lock().await;
        let settings = Arc::make_mut(&mut state.session_configuration.step_settings);
        settings.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
        settings.approvals_reviewer = ApprovalsReviewer::User;
    }
    let original_environments = session.services.turn_environments.selections();
    let workspace = tempfile::tempdir().expect("create proposed workspace");
    let proposed_environments = local_selections(
        AbsolutePathBuf::try_from(workspace.path()).expect("absolute workspace path"),
    );
    assert_ne!(original_environments, proposed_environments.environments);
    let default_mode = session.collaboration_mode().await;
    let overrides = ThreadSettingsOverrides {
        model: Some("automatic-model-must-not-be-applied".to_string()),
        service_tier: Some(Some(ServiceTier::Fast.request_value().to_string())),
        environments: Some(proposed_environments.clone()),
        approval_policy: Some(AskForApproval::Never),
        approvals_reviewer: Some(ApprovalsReviewer::AutoReview),
        ..Default::default()
    };
    let default_override = ThreadSettingsOverrides {
        collaboration_mode: Some(default_mode.clone()),
        ..overrides.clone()
    };
    let prepared = PreparedTurnInputSettings::prepare(
        &session,
        overrides.clone(),
        TurnStartOptions::default(),
    )
    .await
    .expect("sparse settings should preview successfully");
    let prepared_default =
        PreparedTurnInputSettings::prepare(&session, default_override, TurnStartOptions::default())
            .await
            .expect("Default replacement should preview successfully");

    // Another settings writer changes the effective mode after preview. The
    // sparse patch must not commit in Plan, and a full Default replacement
    // must not let automatic work escape the now-current Plan configuration.
    let mut collaboration_mode = default_mode;
    collaboration_mode.mode = ModeKind::Plan;
    session
        .update_settings(SessionSettingsUpdate {
            step_settings: StepSettingsUpdate {
                collaboration_mode: Some(collaboration_mode),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("Plan mode should be allowed for explicit settings updates");
    let desired_settings = session.thread_settings_snapshot().await;
    records.lock().expect("config records lock").clear();
    // Keep the existing refresh worker from consuming invalidation while the
    // rejection and its positive control inspect MCP's dirty state.
    let _mcp_refresh = session
        .mcp_refresh
        .acquire()
        .await
        .expect("acquire MCP refresh gate");
    session.mcp_refresh.claim();
    assert!(!session.mcp_refresh.is_pending());

    for (submission_id, prepared) in [
        ("automatic-after-preview", prepared),
        ("automatic-default-after-preview", prepared_default),
    ] {
        let outcome = prepared
            .apply_started(
                &session,
                submission_id.to_string(),
                TurnStartKind::Automatic,
            )
            .await
            .expect("automatic admission should return a typed rejection");
        assert!(outcome.is_none());
        assert_eq!(session.thread_settings_snapshot().await, desired_settings);
        assert!(session.active_turn.lock().await.is_none());
        assert_eq!(
            session.services.turn_environments.selections(),
            original_environments
        );
        assert!(!session.mcp_refresh.is_pending());
        assert_eq!(*records.lock().expect("config records lock"), Vec::new());
        while let Ok(event) = rx.try_recv() {
            assert_ne!(event.id, submission_id);
        }
    }

    assert_eq!(
        session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await
            .0,
        Vec::<TurnInput>::new()
    );

    // The rejected candidate is valid and would have real runtime effects if
    // accepted by an ordinary settings update.
    session
        .update_settings(thread_settings::prepare_update(overrides))
        .await
        .expect("explicit settings update accepts the same patch");
    assert_eq!(
        session.services.turn_environments.selections(),
        proposed_environments.environments
    );
    assert!(session.mcp_refresh.is_pending());
    assert_eq!(
        *records.lock().expect("config records lock"),
        vec![(AskForApproval::Never, ApprovalsReviewer::AutoReview)]
    );
}

#[test_case(TurnStartKind::User; "ordinary constructor")]
#[test_case(TurnStartKind::Automatic; "conditional constructor")]
#[tokio::test]
async fn admission_revalidates_constraints_before_committing(kind: TurnStartKind) {
    let (session, _turn_context, rx) = make_session_and_context_with_rx().await;
    {
        let mut state = session.state.lock().await;
        Arc::make_mut(&mut state.session_configuration.step_settings).approval_policy =
            Constrained::allow_any(AskForApproval::OnRequest);
    }
    let prepared = PreparedTurnInputSettings::prepare(
        &session,
        ThreadSettingsOverrides {
            approval_policy: Some(AskForApproval::Never),
            service_tier: Some(Some(ServiceTier::Fast.request_value().to_string())),
            ..Default::default()
        },
        TurnStartOptions::default(),
    )
    .await
    .expect("approval-policy edit should initially be valid");

    let approval_policy = Constrained::allow_only(AskForApproval::OnRequest);
    let expected_message = CodexErr::InvalidRequest(
        approval_policy
            .can_set(&AskForApproval::Never)
            .expect_err("new constraint must reject the prepared edit")
            .to_string(),
    )
    .to_string();
    {
        let mut state = session.state.lock().await;
        Arc::make_mut(&mut state.session_configuration.step_settings).approval_policy =
            approval_policy;
    }
    let desired_settings = session.thread_settings_snapshot().await;
    let submission_id = "constraints-after-preview";
    let result = prepared
        .apply_started(&session, submission_id.to_string(), kind)
        .await;
    let Err(error) = result else {
        panic!("commit-time constraint failure must return InvalidRequest");
    };
    let CodexErrorDetails::InvalidRequest(message) = error.details() else {
        panic!("unexpected commit-time error: {error}");
    };
    assert_eq!(message, &expected_message);
    let errors: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|event| event.id == submission_id)
        .map(|event| match event.msg {
            EventMsg::Error(error) => error,
            other => panic!("unexpected rejected-turn event: {other:?}"),
        })
        .collect();
    assert_eq!(
        errors,
        vec![ErrorEvent {
            misalignment: None,
            message: expected_message,
            codex_error_info: Some(CodexErrorInfo::BadRequest),
        }]
    );
    assert_eq!(session.thread_settings_snapshot().await, desired_settings);
    assert!(session.active_turn.lock().await.is_none());
    assert_eq!(
        session
            .input_queue
            .get_pending_input(&session.active_turn)
            .await
            .0,
        Vec::<TurnInput>::new()
    );
}

#[tokio::test]
async fn start_only_accepts_user_input_in_plan_mode() {
    let (session, _turn_context, _rx) = make_session_and_context_with_rx().await;
    let mut collaboration_mode = session.collaboration_mode().await;
    collaboration_mode.mode = ModeKind::Plan;
    {
        let mut state = session.state.lock().await;
        Arc::make_mut(&mut state.session_configuration.step_settings).collaboration_mode =
            collaboration_mode;
        state.merge_connector_selection(["calendar".to_string()]);
    }

    let submission = submit_start_only(
        &session,
        SubmittedTurnInput::UserInput {
            content: vec![UserInput::Text {
                text: "queued user input".to_string(),
                text_elements: Vec::new(),
            }],
            client_id: Some("queued-user-message".to_string()),
        },
    )
    .await;
    assert!(matches!(submission, TurnInputSubmission::Started { .. }));
    assert!(
        session
            .state
            .lock()
            .await
            .get_connector_selection()
            .is_empty()
    );

    session.abort_all_tasks(TurnAbortReason::Interrupted).await;
}

#[tokio::test]
async fn start_only_rejects_empty_user_input_in_plan_mode() {
    let (session, _turn_context, _rx) = make_session_and_context_with_rx().await;
    let mut collaboration_mode = session.collaboration_mode().await;
    collaboration_mode.mode = ModeKind::Plan;
    {
        let mut state = session.state.lock().await;
        Arc::make_mut(&mut state.session_configuration.step_settings).collaboration_mode =
            collaboration_mode;
    }

    let submission = submit_start_only(
        &session,
        SubmittedTurnInput::UserInput {
            content: Vec::new(),
            client_id: Some("empty-queued-user-message".to_string()),
        },
    )
    .await;

    assert_eq!(
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::PlanMode,
        },
        submission
    );
    assert!(session.active_turn.lock().await.is_none());
}

#[tokio::test]
async fn start_only_rejects_pending_trigger_turn_without_injecting() {
    let (session, _turn_context, _rx) = make_session_and_context_with_rx().await;
    {
        let mut state = session.state.lock().await;
        Arc::make_mut(&mut state.session_configuration.step_settings)
            .collaboration_mode
            .mode = ModeKind::Plan;
    }
    session
        .input_queue
        .enqueue_mailbox_communication(
            InterAgentCommunication::new(
                AgentPath::root(),
                AgentPath::root(),
                Vec::new(),
                "pending trigger".to_string(),
                /*trigger_turn*/ true,
            ),
            Default::default(),
        )
        .await;

    let submission = submit_start_only(
        &session,
        SubmittedTurnInput::ResponseItem(user_message("synthetic idle input")),
    )
    .await;

    assert_eq!(
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::PendingTriggerTurn,
        },
        submission
    );
    assert!(session.active_turn.lock().await.is_none());
    assert!(session.input_queue.has_trigger_turn_mailbox_items().await);
    assert_eq!(session.collaboration_mode().await.mode, ModeKind::Plan);
}

#[tokio::test]
async fn steer_only_requires_active_turn() {
    let (session, _turn_context, _rx) = make_session_and_context_with_rx().await;
    let submission = submit_steer_only(
        &session,
        vec![UserInput::Text {
            text: "steer".to_string(),
            text_elements: Vec::new(),
        }],
        "missing-turn-id",
    )
    .await;

    assert_eq!(
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::NoActiveTurn,
        },
        submission
    );
}

#[tokio::test]
async fn steer_only_enforces_expected_turn_id() {
    let (session, turn_context, _rx) = make_session_and_context_with_rx().await;
    session
        .spawn_task(
            Arc::clone(&turn_context),
            vec![TurnInput::UserInput {
                content: vec![UserInput::Text {
                    text: "hello".to_string(),
                    text_elements: Vec::new(),
                }],
                client_id: None,
            }],
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: false,
            },
        )
        .await;

    let submission = submit_steer_only(
        &session,
        vec![UserInput::Text {
            text: "steer".to_string(),
            text_elements: Vec::new(),
        }],
        "different-turn-id",
    )
    .await;
    assert_eq!(
        TurnInputSubmission::NotSubmitted {
            reason: NotSubmittedReason::ExpectedTurnMismatch {
                expected: "different-turn-id".to_string(),
                actual: turn_context.sub_id.clone(),
            },
        },
        submission
    );

    let output: ResponseItem = serde_json::from_value(serde_json::json!({
        "type": "function_call_output",
        "name": "send_message_to_thread",
        "output": "delegated work",
    }))
    .expect("valid standalone output");

    let submission = handle(
        &session,
        TurnInputRequest::new(SubmittedTurnInput::ResponseItem(output)),
        TurnInputMode::StartOrSteer,
        "test-submission".to_string(),
    )
    .await
    .expect("standalone output should steer the active turn");

    assert_eq!(
        submission,
        TurnInputSubmission::Steered {
            turn_id: turn_context.sub_id.clone()
        }
    );
    let turn_state = session
        .input_queue
        .turn_state_for_sub_id(&session.active_turn, &turn_context.sub_id)
        .await
        .expect("active turn state");
    assert_eq!(
        session
            .input_queue
            .subscribe_activity(Some(turn_state.as_ref()))
            .await
            .1,
        Some(crate::session::input_queue::InputQueueActivity::Steer)
    );
}

#[tokio::test]
async fn rejects_non_regular_turns() {
    for (task_kind, turn_kind) in [
        (TaskKind::Review, NonSteerableTurnKind::Review),
        (TaskKind::Compact, NonSteerableTurnKind::Compact),
    ] {
        let (session, incoming_turn_context, _rx) = make_session_and_context_with_rx().await;
        incoming_turn_context
            .turn_metadata_state
            .set_root_turn_id("incoming-root".to_string());
        let turn_context = session
            .new_turn_with_default_settings("turn".to_string(), Default::default())
            .await;
        turn_context
            .turn_metadata_state
            .set_root_turn_id("active-root".to_string());
        session
            .spawn_task(
                Arc::clone(&turn_context),
                vec![TurnInput::UserInput {
                    content: vec![UserInput::Text {
                        text: "hello".to_string(),
                        text_elements: Vec::new(),
                    }],
                    client_id: None,
                }],
                NeverEndingTask {
                    kind: task_kind,
                    listen_to_cancellation_token: true,
                },
            )
            .await;

        let steer_input = vec![UserInput::Text {
            text: "steer".to_string(),
            text_elements: Vec::new(),
        }];
        let steer_submission = submit_steer_only(&session, steer_input.clone(), "turn").await;
        assert_eq!(
            TurnInputSubmission::NotSubmitted {
                reason: NotSubmittedReason::ActiveTurnNotSteerable { turn_kind },
            },
            steer_submission
        );
        let start_or_steer_submission = handle(
            &session,
            TurnInputRequest::user_input(steer_input),
            TurnInputMode::StartOrSteer,
            "test-submission".to_string(),
        )
        .await
        .expect("start-or-steer submission should be valid");
        assert_eq!(
            TurnInputSubmission::NotSubmitted {
                reason: NotSubmittedReason::ActiveTurnNotSteerable { turn_kind },
            },
            start_or_steer_submission
        );
        assert_eq!(
            turn_context.turn_metadata_state.root_turn_id().as_deref(),
            Some("active-root")
        );

        session.abort_all_tasks(TurnAbortReason::Interrupted).await;
    }
}
