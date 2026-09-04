use super::StateRuntime;
use super::test_support::unique_temp_dir;
use codex_utils_absolute_path::test_support::PathExt;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) struct StateRuntimeTestFixture {
    runtime: Arc<StateRuntime>,
    codex_home: PathBuf,
}

impl std::ops::Deref for StateRuntimeTestFixture {
    type Target = Arc<StateRuntime>;

    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

impl StateRuntimeTestFixture {
    pub(super) async fn new() -> Self {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await
        .expect("state db should initialize");
        Self {
            runtime,
            codex_home,
        }
    }

    pub(super) async fn teardown(self) {
        self.runtime.close().await;
        drop(self.runtime);
        tokio::fs::remove_dir_all(&self.codex_home)
            .await
            .unwrap_or_else(|err| panic!("remove {}: {err}", self.codex_home.display()));
        assert!(
            !tokio::fs::try_exists(&self.codex_home)
                .await
                .unwrap_or_else(|err| panic!("inspect {}: {err}", self.codex_home.display())),
            "state runtime fixture root must be absent after teardown: {}",
            self.codex_home.display()
        );
    }
}
