use super::*;
use pretty_assertions::assert_eq;

async fn pause_at(
    hook: &LifecycleCommitTestHook,
    child_id: ThreadId,
) -> (Arc<Barrier>, Arc<Notify>) {
    let (entered, release) = (Arc::new(Barrier::new(2)), Arc::new(Notify::new()));
    *hook.lock().await = Some((child_id, Arc::clone(&entered), Arc::clone(&release)));
    (entered, release)
}

fn relative_path_snapshot(root: &std::path::Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|err| panic!("read {}: {err}", directory.display()));
        for entry in entries {
            let path = entry.expect("directory entry should be readable").path();
            paths.push(
                path.strip_prefix(root)
                    .expect("snapshot path should remain under its root")
                    .to_path_buf(),
            );
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    paths.sort();
    paths
}

#[tokio::test]
async fn non_ephemeral_thread_spawn_without_graph_store_fails_before_session_spawn() {
    let temp_dir = tempdir().expect("tempdir");
    let mut config = test_config().await;
    config.codex_home = temp_dir.path().join("codex-home").abs();
    config.cwd = config.codex_home.abs();
    config.ephemeral = false;
    std::fs::create_dir_all(&config.codex_home).expect("create codex home");
    let _ = config.features.enable(Feature::MultiAgentV2);

    let root_id = ThreadId::from_u128(/*value*/ 0x018f_0000_0000_7000_8000_0000_0000_0011);
    let child_id = ThreadId::from_u128(/*value*/ 0x018f_0000_0000_7000_8000_0000_0000_0012);
    let generated_ids = [root_id, child_id];
    let next_id = std::sync::atomic::AtomicUsize::new(0);
    let manager = Arc::new(
        ThreadManager::with_models_provider_and_home_for_tests(
            CodexAuth::from_api_key("dummy"),
            config.model_provider.clone(),
            config.codex_home.to_path_buf(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        )
        .with_thread_id_generator(move || generated_ids[next_id.fetch_add(1, Ordering::Relaxed)]),
    );
    let root = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .expect("root thread should not require a spawn edge store");
    root.thread.ensure_rollout_materialized().await;
    root.thread
        .flush_rollout()
        .await
        .expect("root rollout should flush before the filesystem snapshot");
    let paths_before_spawn = relative_path_snapshot(&config.codex_home);
    let mut created = manager.subscribe_thread_created();
    let (entered, release) = pause_at(
        &manager.state.registration_before_lifecycle_commit_hook,
        child_id,
    )
    .await;
    let control = root.thread.session.services.agent_control.clone();
    let spawn_control = control.clone();
    let spawn_config = config.clone();
    let spawn_source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id: root_id,
        depth: 1,
        agent_path: None,
        agent_nickname: None,
        agent_role: None,
    });
    let mut spawn = tokio::spawn(async move {
        spawn_control
            .spawn_agent_with_metadata(
                spawn_config,
                vec![UserInput::Text {
                    text: "must never reach the child input queue".to_string(),
                    text_elements: Vec::new(),
                }],
                Some(spawn_source),
                SpawnAgentOptions {
                    parent_thread_id: Some(root_id),
                    ..Default::default()
                },
            )
            .await
    });

    let result = tokio::select! {
        result = &mut spawn => result.expect("spawn task should join"),
        _ = entered.wait() => {
            let transient_registration = manager.get_thread(child_id).await.is_ok();
            let transient_metadata = control.ensure_agent_known(child_id).is_ok();
            let transient_paths = relative_path_snapshot(&config.codex_home);
            release.notify_one();
            let rollback_result = spawn.await.expect("spawn rollback task should join");
            panic!(
                "spawn reached the post-registration hook before rejecting the missing graph store: registration={transient_registration}, metadata={transient_metadata}, new_paths={:?}, rollback_result={rollback_result:?}",
                transient_paths
                    .iter()
                    .filter(|path| !paths_before_spawn.contains(path))
                    .collect::<Vec<_>>()
            );
        }
    };
    *manager
        .state
        .registration_before_lifecycle_commit_hook
        .lock()
        .await = None;

    let failure = result
        .expect_err("missing graph store must reject durable child spawn")
        .to_string();
    assert!(failure.contains("graph store unavailable"));
    assert!(manager.get_thread(child_id).await.is_err());
    assert!(control.ensure_agent_known(child_id).is_err());
    assert!(matches!(
        created.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(
        relative_path_snapshot(&config.codex_home),
        paths_before_spawn,
        "rejection must not create a child session, rollout, or persisted initial input"
    );
    let report = manager
        .shutdown_all_threads_bounded(Duration::from_secs(10))
        .await;
    assert!(report.submit_failed.is_empty());
    assert!(report.timed_out.is_empty());
}
