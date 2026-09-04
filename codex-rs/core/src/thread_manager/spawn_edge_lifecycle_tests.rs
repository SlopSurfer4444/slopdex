use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn removing_exact_child_runtime_preserves_its_open_spawn_edge() {
    let temp_dir = tempdir().expect("tempdir");
    let mut config = test_config().await;
    config.codex_home = temp_dir.path().join("codex-home").abs();
    config.cwd = config.codex_home.abs();
    std::fs::create_dir_all(&config.codex_home).expect("create codex home");
    let _ = config.features.enable(Feature::MultiAgentV2);
    let state_db = init_state_db(&config).await;
    let manager = ThreadManager::with_models_provider_home_and_state_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.clone().to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        state_db.clone(),
    );
    let root = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .expect("start root thread");
    let child = root
        .thread
        .session
        .services
        .agent_control
        .spawn_agent_with_metadata(
            config,
            vec![UserInput::Text {
                text: "child task".to_string(),
                text_elements: Vec::new(),
            }],
            Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id: root.thread_id,
                depth: 1,
                agent_path: None,
                agent_nickname: None,
                agent_role: None,
            })),
            SpawnAgentOptions {
                parent_thread_id: Some(root.thread_id),
                ..Default::default()
            },
        )
        .await
        .expect("spawn child thread");

    let child_thread = manager
        .get_thread(child.thread_id)
        .await
        .expect("child runtime should be registered");
    let removed = manager
        .remove_thread_if_matches(&child.thread_id, &child_thread)
        .await;
    assert!(
        removed.is_some(),
        "the exact child runtime should be removed"
    );

    let graph_store = manager
        .state
        .agent_graph_store()
        .expect("state-backed manager should expose graph store");
    assert_eq!(
        graph_store
            .list_thread_spawn_children(
                root.thread_id,
                Some(codex_agent_graph_store::ThreadSpawnEdgeStatus::Open),
            )
            .await
            .expect("open child edge should be queryable"),
        vec![child.thread_id]
    );
}

struct FailOpenStore {
    inner: Arc<dyn GraphStore>,
    fail_open: std::sync::atomic::AtomicBool,
}
macro_rules! forward_graph_store {
    ($method:ident($($argument:ident: $argument_type:ty),*) -> $output:ty) => {
        fn $method(&self, $($argument: $argument_type),*) -> Fut<'_, $output> {
            self.inner.$method($($argument),*)
        }
    };
}
impl GraphStore for FailOpenStore {
    fn upsert_thread_spawn_edge(&self, p: ThreadId, c: ThreadId, s: EdgeStatus) -> Fut<'_, ()> {
        if s == EdgeStatus::Open && self.fail_open.swap(false, Ordering::SeqCst) {
            const MESSAGE: &str = "injected open-edge write failure";
            let message = MESSAGE.to_string();
            return Box::pin(async move { Err(Error::Internal { message }) });
        }
        self.inner.upsert_thread_spawn_edge(p, c, s)
    }
    forward_graph_store!(set_thread_spawn_edge_status(child: ThreadId, status: EdgeStatus) -> ());
    forward_graph_store!(close_open_thread_spawn_edge(parent: ThreadId, child: ThreadId) -> codex_agent_graph_store::ThreadSpawnEdgeCloseOutcome);
    forward_graph_store!(list_thread_spawn_children(parent: ThreadId, status: Option<EdgeStatus>) -> Vec<ThreadId>);
    forward_graph_store!(list_thread_spawn_descendants(root: ThreadId, status: Option<EdgeStatus>) -> Vec<ThreadId>);
}
struct LifeFixture {
    _temp: tempfile::TempDir,
    config: Config,
    manager: Arc<ThreadManager>,
    root: ThreadId,
    control: AgentControl,
    store: Arc<FailOpenStore>,
}
// Snapshot order: open edges, closed edges, runtime present, registry known.
type LifeSnapshot = (Vec<ThreadId>, Vec<ThreadId>, bool, bool);
enum LifeState {
    LiveOpen,
    OpenGone,
    LiveClosed,
    ClosedGone,
    Absent,
}
impl LifeState {
    fn snapshot(self, child_id: ThreadId) -> LifeSnapshot {
        match self {
            Self::LiveOpen => (vec![child_id], vec![], true, true),
            Self::OpenGone => (vec![child_id], vec![], false, true),
            Self::LiveClosed => (vec![], vec![child_id], true, true),
            Self::ClosedGone => (vec![], vec![child_id], false, false),
            Self::Absent => (vec![], vec![], false, false),
        }
    }
}
async fn pause_at(
    hook: &LifecycleCommitTestHook,
    child_id: ThreadId,
) -> (Arc<Barrier>, Arc<Notify>) {
    let (entered, release) = (Arc::new(Barrier::new(2)), Arc::new(Notify::new()));
    *hook.lock().await = Some((child_id, Arc::clone(&entered), Arc::clone(&release)));
    (entered, release)
}
async fn assert_current(manager: &ThreadManager, id: ThreadId, thread: &Arc<CodexThread>) {
    let current = manager.get_thread(id).await.expect("current runtime");
    assert!(Arc::ptr_eq(&current, thread));
}
impl LifeFixture {
    async fn new() -> Self {
        let temp_dir = tempdir().expect("tempdir");
        let mut config = test_config().await;
        config.codex_home = temp_dir.path().join("codex-home").abs();
        config.cwd = config.codex_home.abs();
        std::fs::create_dir_all(&config.codex_home).expect("create codex home");
        let _ = config.features.enable(Feature::MultiAgentV2);
        let state_db = init_state_db(&config).await;
        let mut manager = ThreadManager::with_models_provider_home_and_state_for_tests(
            CodexAuth::from_api_key("dummy"),
            config.model_provider.clone(),
            config.codex_home.clone().to_path_buf(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
            state_db,
        );
        let inner = manager.state.agent_graph_store().expect("graph store");
        let store = Arc::new(FailOpenStore {
            inner,
            fail_open: std::sync::atomic::AtomicBool::new(false),
        });
        let state = Arc::get_mut(&mut manager.state).expect("unshared manager state");
        state.agent_graph_store = Some(store.clone());
        let manager = Arc::new(manager);
        let root = manager
            .start_thread(StartThreadOptions::new(config.clone()))
            .await
            .expect("start root");
        let control = root.thread.session.services.agent_control.clone();
        Self {
            _temp: temp_dir,
            config,
            manager,
            root: root.thread_id,
            control,
            store,
        }
    }
    fn source(&self) -> SessionSource {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id: self.root,
            depth: 1,
            agent_path: None,
            agent_nickname: None,
            agent_role: None,
        })
    }
    async fn children(&self, status: EdgeStatus) -> Vec<ThreadId> {
        self.store
            .inner
            .list_thread_spawn_children(self.root, Some(status))
            .await
            .expect("thread-spawn edges")
    }
    async fn snapshot(&self, child_id: ThreadId) -> LifeSnapshot {
        (
            self.children(EdgeStatus::Open).await,
            self.children(EdgeStatus::Closed).await,
            self.manager.state.get_thread(child_id).await.is_ok(),
            self.control.ensure_agent_known(child_id).is_ok(),
        )
    }
    async fn assert_life(&self, child_id: ThreadId, expected: LifeState) {
        assert_eq!(self.snapshot(child_id).await, expected.snapshot(child_id));
    }
    async fn persist(thread: &CodexThread) {
        thread.ensure_rollout_materialized().await;
        thread.flush_rollout().await.expect("flush rollout");
    }
    async fn remove(&self, child_id: ThreadId, thread: &Arc<CodexThread>) {
        let removed = self
            .manager
            .remove_thread_if_matches(&child_id, thread)
            .await;
        assert!(removed.is_some());
    }
    async fn stop_remove(&self, child_id: ThreadId, thread: &Arc<CodexThread>) {
        thread.shutdown_and_wait().await.expect("shutdown");
        self.remove(child_id, thread).await;
    }
    async fn resume(&self, child_id: ThreadId) -> CodexResult<ThreadId> {
        self.control
            .resume_agent_from_rollout(self.config.clone(), child_id, self.source())
            .await
    }
    fn close(&self, child_id: ThreadId) -> tokio::task::JoinHandle<CodexResult<String>> {
        let control = self.control.clone();
        tokio::spawn(async move { control.close_agent(child_id).await })
    }
    async fn spawn(&self) -> (ThreadId, Arc<CodexThread>) {
        let child = self
            .control
            .spawn_agent_with_metadata(
                self.config.clone(),
                vec![UserInput::Text {
                    text: "child".to_string(),
                    text_elements: Vec::new(),
                }],
                Some(self.source()),
                SpawnAgentOptions {
                    parent_thread_id: Some(self.root),
                    ..Default::default()
                },
            )
            .await
            .expect("spawn child");
        let child_id = child.thread_id;
        (
            child_id,
            self.manager
                .get_thread(child_id)
                .await
                .expect("child runtime"),
        )
    }
}
#[tokio::test]
async fn explicit_close_racing_runtime_unload_closes_exact_edge_and_registry() {
    let fixture = LifeFixture::new().await;
    let (child_id, child) = fixture.spawn().await;
    assert_current(&fixture.manager, child_id, &child).await;
    fixture.assert_life(child_id, LifeState::LiveOpen).await;
    let hook = &fixture.manager.state.close_before_lifecycle_commit_hook;
    let (entered, release) = pause_at(hook, child_id).await;
    let close = fixture.close(child_id);
    entered.wait().await;
    fixture.remove(child_id, &child).await;
    fixture.assert_life(child_id, LifeState::OpenGone).await;
    release.notify_one();
    assert!(close.await.expect("close task").is_ok());
    *hook.lock().await = None;
    fixture.assert_life(child_id, LifeState::ClosedGone).await;
}
#[tokio::test]
async fn cancelled_explicit_close_finishes_exact_lifecycle_commit() {
    let fixture = LifeFixture::new().await;
    let (child_id, child) = fixture.spawn().await;
    fixture.assert_life(child_id, LifeState::LiveOpen).await;
    assert_current(&fixture.manager, child_id, &child).await;
    let (entered, release) = (Arc::new(Barrier::new(2)), Arc::new(Notify::new()));
    let (probe, receiver) =
        CloseAfterDurableEdgeTestProbe::new(child_id, Arc::clone(&entered), Arc::clone(&release));
    let probe_slot = &fixture.manager.state.close_after_durable_edge_probe;
    *probe_slot.lock().await = Some(probe);
    let close = fixture.close(child_id);
    entered.wait().await;
    fixture.assert_life(child_id, LifeState::LiveClosed).await;
    assert_current(&fixture.manager, child_id, &child).await;
    close.abort();
    assert!(close.await.expect_err("close should cancel").is_cancelled());
    release.notify_one();
    assert!(receiver.await.is_ok());
    fixture.assert_life(child_id, LifeState::ClosedGone).await;
}
#[tokio::test]
async fn resume_registration_cannot_reopen_edge_after_concurrent_explicit_close() {
    let fixture = LifeFixture::new().await;
    let (child_id, child_thread) = fixture.spawn().await;
    LifeFixture::persist(&child_thread).await;
    fixture.stop_remove(child_id, &child_thread).await;
    fixture.assert_life(child_id, LifeState::OpenGone).await;
    let state = &fixture.manager.state;
    let hook = &state.registration_before_lifecycle_commit_hook;
    let (entered, release) = pause_at(hook, child_id).await;
    let resume_control = fixture.control.clone();
    let resume_config = fixture.config.clone();
    let resume_source = fixture.source();
    let resume = tokio::spawn(async move {
        resume_control
            .resume_agent_from_rollout(resume_config, child_id, resume_source)
            .await
    });
    entered.wait().await;
    assert!(!resume.is_finished());
    assert!(fixture.control.close_agent(child_id).await.is_ok());
    fixture.assert_life(child_id, LifeState::ClosedGone).await;
    release.notify_one();
    assert!(resume.await.expect("resume task").is_err());
    *hook.lock().await = None;
    fixture.assert_life(child_id, LifeState::ClosedGone).await;
}
#[tokio::test]
async fn stale_close_cannot_close_unloaded_replacement_incarnation() {
    let fixture = LifeFixture::new().await;
    let (child_id, old) = fixture.spawn().await;
    LifeFixture::persist(&old).await;
    let hook = &fixture.manager.state.close_before_lifecycle_commit_hook;
    let (entered, release) = pause_at(hook, child_id).await;
    let close = fixture.close(child_id);
    entered.wait().await;
    fixture.remove(child_id, &old).await;
    fixture.resume(child_id).await.expect("resume replacement");
    let replacement = fixture
        .manager
        .get_thread(child_id)
        .await
        .expect("replacement runtime");
    fixture.stop_remove(child_id, &replacement).await;
    release.notify_one();
    assert!(close.await.expect("close task").is_err());
    fixture.assert_life(child_id, LifeState::OpenGone).await;
}
#[tokio::test]
async fn resume_durable_open_edge_failure_rolls_back_before_exposure() {
    let fixture = LifeFixture::new().await;
    let seeded = fixture
        .manager
        .start_thread(StartThreadOptions::new(fixture.config.clone()))
        .await
        .expect("seed rollout");
    let child_id = seeded.thread_id;
    LifeFixture::persist(&seeded.thread).await;
    fixture.stop_remove(child_id, &seeded.thread).await;
    fixture.assert_life(child_id, LifeState::Absent).await;
    let mut created = fixture.manager.subscribe_thread_created();
    let fail_open = &fixture.store.fail_open;
    fail_open.store(true, Ordering::SeqCst);
    let result = fixture.resume(child_id).await;
    let final_state = fixture.snapshot(child_id).await;
    let failure = result
        .expect_err("durable open-edge failure must surface")
        .to_string();
    if let Ok(runtime) = fixture.manager.get_thread(child_id).await {
        fixture.stop_remove(child_id, &runtime).await;
    }
    assert!(failure.contains("persist thread-spawn edge"));
    assert!(!fail_open.load(Ordering::SeqCst));
    assert_eq!(created.try_recv().ok(), None);
    assert_eq!(final_state, LifeState::Absent.snapshot(child_id));
}
#[tokio::test]
async fn duplicate_thread_shutdown_releases_registration_fence_before_wait() {
    let mut config = test_config().await;
    config.ephemeral = true;
    let manager = Arc::new(ThreadManager::with_models_provider_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
    ));
    let thread_id = manager.reserve_thread_id();
    let mut incumbent_options = StartThreadOptions::new(config.clone());
    incumbent_options.reserved_thread_id = Some(thread_id);
    let incumbent = manager
        .start_thread(incumbent_options)
        .await
        .expect("incumbent");
    let incumbent_thread = Arc::clone(&incumbent.thread);
    let hook = &manager.state.duplicate_before_shutdown_hook;
    let (entered, release) = pause_at(hook, thread_id).await;
    let duplicate_manager = Arc::clone(&manager);
    let duplicate_config = config.clone();
    let mut duplicate = tokio::spawn(async move {
        let mut options = StartThreadOptions::new(duplicate_config);
        options.reserved_thread_id = Some(thread_id);
        duplicate_manager.start_thread(options).await
    });
    tokio::time::timeout(Duration::from_secs(10), entered.wait())
        .await
        .expect("duplicate hook should be reached");
    assert!(!duplicate.is_finished());
    assert_current(&manager, thread_id, &incumbent_thread).await;
    let fence_available = manager.state.thread_registration_fence.try_lock().is_ok();
    release.notify_one();
    let duplicate_result = tokio::time::timeout(Duration::from_secs(10), &mut duplicate)
        .await
        .expect("duplicate cleanup should finish");
    let duplicate_error = duplicate_result
        .expect("duplicate task join")
        .err()
        .expect("duplicate reserved ID should fail");
    assert!(matches!(
        duplicate_error.details(),
        codex_protocol::error::CodexErrorDetails::InvalidRequest(message)
            if message == &format!("thread {thread_id} is already running")
    ));
    assert_current(&manager, thread_id, &incumbent_thread).await;
    *hook.lock().await = None;
    incumbent_thread
        .shutdown_and_wait()
        .await
        .expect("shutdown");
    assert!(
        fence_available,
        "duplicate shutdown held registration fence"
    );
}
#[tokio::test]
async fn removing_child_runtime_does_not_require_incoming_edge_parent_match() {
    let temp_dir = tempdir().expect("tempdir");
    let mut config = test_config().await;
    config.codex_home = temp_dir.path().join("codex-home").abs();
    config.cwd = config.codex_home.abs();
    std::fs::create_dir_all(&config.codex_home).expect("create codex home");
    let _ = config.features.enable(Feature::MultiAgentV2);
    let state_db = init_state_db(&config).await;
    let manager = ThreadManager::with_models_provider_home_and_state_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.clone().to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        state_db.clone(),
    );
    let root = manager
        .start_thread(StartThreadOptions::new(config.clone()))
        .await
        .expect("start root thread");
    let child = root
        .thread
        .session
        .services
        .agent_control
        .spawn_agent_with_metadata(
            config,
            vec![UserInput::Text {
                text: "child task".to_string(),
                text_elements: Vec::new(),
            }],
            Some(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                parent_thread_id: root.thread_id,
                depth: 1,
                agent_path: None,
                agent_nickname: None,
                agent_role: None,
            })),
            SpawnAgentOptions {
                parent_thread_id: Some(root.thread_id),
                ..Default::default()
            },
        )
        .await
        .expect("spawn child thread");
    let other_parent_thread_id = ThreadId::new();
    state_db
        .as_ref()
        .expect("state db should be available")
        .upsert_thread_spawn_edge(
            other_parent_thread_id,
            child.thread_id,
            codex_state::DirectionalThreadSpawnEdgeStatus::Open,
        )
        .await
        .expect("replace child edge with mismatched parent");

    let child_thread = manager
        .get_thread(child.thread_id)
        .await
        .expect("child runtime should be registered");
    let removed = manager
        .remove_thread_if_matches(&child.thread_id, &child_thread)
        .await;
    assert!(removed.is_some(), "exact runtime removal should succeed");
    assert!(manager.state.get_thread(child.thread_id).await.is_err());
}

#[tokio::test]
async fn bulk_shutdown_does_not_remove_same_id_replacements() {
    let config = test_config().await;
    let manager = Arc::new(ThreadManager::with_models_provider_and_home_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        config.codex_home.to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    ));

    for _ in 0..2 {
        let old = manager
            .start_thread(StartThreadOptions::new(config.clone()))
            .await
            .expect("start old runtime");
        let snapshot_ready = Arc::new(tokio::sync::Barrier::new(2));
        let resume_cleanup = Arc::new(tokio::sync::Notify::new());
        *manager.state.shutdown_all_threads_hook.lock().await =
            Some((Arc::clone(&snapshot_ready), Arc::clone(&resume_cleanup)));
        let shutdown_manager = Arc::clone(&manager);
        let shutdown = tokio::spawn(async move {
            shutdown_manager
                .shutdown_all_threads_bounded(Duration::from_secs(10))
                .await
        });
        snapshot_ready.wait().await;
        {
            let mut threads = manager.state.threads.write().await;
            assert!(threads.remove(&old.thread_id).is_some());
        }

        let replacement = manager
            .start_thread(StartThreadOptions {
                reserved_thread_id: Some(old.thread_id),
                ..StartThreadOptions::new(config.clone())
            })
            .await
            .expect("start same-id replacement");
        resume_cleanup.notify_one();
        let report = shutdown.await.expect("bulk shutdown task should complete");
        assert!(report.completed.is_empty());
        assert_eq!(report.submit_failed, vec![old.thread_id]);
        assert!(report.timed_out.is_empty());
        let current = manager
            .state
            .get_thread(old.thread_id)
            .await
            .expect("replacement should remain registered");
        assert!(Arc::ptr_eq(&current, &replacement.thread));
        *manager.state.shutdown_all_threads_hook.lock().await = None;
        replacement
            .thread
            .shutdown_and_wait()
            .await
            .expect("replacement should stop");
        assert!(
            manager
                .remove_thread_if_matches(&old.thread_id, &replacement.thread)
                .await
                .is_some()
        );
    }
}
