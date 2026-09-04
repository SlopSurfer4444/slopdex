use super::*;

#[tokio::test]
async fn targetful_wait_registration_refusal_emits_no_in_progress_activity() {
    let (mut session, mut turn, events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let mut config = (*turn.config).clone();
    config.ephemeral = true;
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("test config should allow feature update");
    set_turn_config(
        Arc::get_mut(&mut turn).expect("unique turn"),
        config.clone(),
    );
    let manager = thread_manager().await;
    let root = manager
        .start_thread(StartThreadOptions::new(config))
        .await
        .expect("root thread should start");
    let session_mut = Arc::get_mut(&mut session).expect("unique session");
    session_mut.services.agent_control = manager.agent_control();
    session_mut.thread_id = root.thread_id;

    SpawnAgentHandlerV2::default()
        .handle(invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "spawn_agent",
            function_payload(json!({
                "message": "boot worker",
                "task_name": "worker",
                "fork_turns": "none"
            })),
        ))
        .await
        .expect("worker should spawn");
    let child_id = session
        .services
        .agent_control
        .resolve_agent_reference(session.thread_id, &turn.session_source, "worker")
        .await
        .expect("worker should resolve");
    let child = manager
        .get_thread(child_id)
        .await
        .expect("worker runtime should exist");
    child
        .session
        .abort_all_tasks(TurnAbortReason::Interrupted)
        .await;
    timeout(Duration::from_secs(2), async {
        while child.session.active_turn.lock().await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker turn should become unbound");
    while events.try_recv().is_ok() {}

    let Err(error) = WaitAgentHandlerV2::default()
        .handle(invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "wait_agent",
            function_payload(json!({
                "targets": [child_id.to_string()],
                "timeout_ms": 1
            })),
        ))
        .await
    else {
        panic!("unbound child turn should refuse targetful registration");
    };
    assert!(matches!(
        error,
        FunctionCallError::RespondToModel(message)
            if message.contains("could not bind the requested current child turns")
    ));
    assert!(
        events.try_recv().is_err(),
        "registration refusal must not leave a visible in-progress wait activity"
    );
}

#[tokio::test]
async fn root_join_rejects_an_existing_grandchild_at_the_handler_boundary() {
    let (mut session, mut turn, _events) =
        crate::session::tests::make_session_and_context_with_rx().await;
    let mut config = (*turn.config).clone();
    config.ephemeral = true;
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("test config should allow feature update");
    set_turn_config(
        Arc::get_mut(&mut turn).expect("unique turn"),
        config.clone(),
    );
    let manager = thread_manager().await;
    let root = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .expect("root thread should start");
    let session_mut = Arc::get_mut(&mut session).expect("unique session");
    session_mut.services.agent_control = manager.agent_control();
    session_mut.thread_id = root.thread_id;

    SpawnAgentHandlerV2::default()
        .handle(invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "spawn_agent",
            function_payload(json!({
                "message": "boot child",
                "task_name": "child",
                "fork_turns": "none"
            })),
        ))
        .await
        .expect("child should spawn");
    let child_id = session
        .services
        .agent_control
        .resolve_agent_reference(session.thread_id, &turn.session_source, "child")
        .await
        .expect("child should resolve");
    let child = manager
        .get_thread(child_id)
        .await
        .expect("child runtime should exist");
    let grandchild_source = thread_spawn_source(
        child_id,
        &child.session_source,
        crate::agent::next_thread_spawn_depth(&child.session_source),
        /*agent_role*/ None,
        Some("grandchild".to_string()),
    )
    .expect("grandchild source should be pathful");
    let grandchild_id = session
        .services
        .agent_control
        .spawn_agent_with_metadata(
            config,
            vec![UserInput::Text {
                text: "boot grandchild".to_string(),
                text_elements: Vec::new(),
            }],
            Some(grandchild_source),
            crate::agent::control::SpawnAgentOptions {
                parent_thread_id: Some(child_id),
                ..Default::default()
            },
        )
        .await
        .expect("grandchild should spawn")
        .thread_id;

    let Err(FunctionCallError::RespondToModel(message)) = JoinAgentsHandlerV2
        .handle(invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "join_agents",
            function_payload(json!({
                "targets": [grandchild_id.to_string()],
                "condition": "all"
            })),
        ))
        .await
    else {
        panic!("root must not join its existing grandchild directly");
    };
    assert!(message.contains("is not a direct child of /root"));
}
