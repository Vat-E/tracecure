//! `AppState` — thin wrapper around `tracecure_storage::Store` that the axum
//! handlers receive via `State<AppState>`.

use std::path::Path;
use std::sync::Arc;

use tracecure_storage::{Store, StoreConfig};

/// Cheap-to-clone handle shared by all axum handlers.
#[derive(Clone)]
pub struct AppState {
    store: Arc<Store>,
}

impl AppState {
    /// Open the storage at `data_dir`. Creates parent directories as needed;
    /// WAL + segments subdirectories are created inside `data_dir`.
    pub async fn open(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::open_with(data_dir, StoreConfig::default()).await
    }

    /// Open with an explicit storage configuration. The server uses this to
    /// pass the retention rules from its command line; every other field is
    /// left at its default there.
    pub async fn open_with(
        data_dir: impl AsRef<Path>,
        config: StoreConfig,
    ) -> anyhow::Result<Self> {
        let store = Store::open_with(data_dir, config).await?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }
}
