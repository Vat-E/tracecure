//! `AppState` — thin wrapper around `tracecure_storage::Store` that the axum
//! handlers receive via `State<AppState>`.

use std::path::Path;
use std::sync::Arc;

use tracecure_storage::{Store, StoreConfig};

use crate::auth::TokenStore;

/// Cheap-to-clone handle shared by all axum handlers.
#[derive(Clone)]
pub struct AppState {
    store: Arc<Store>,
    /// Set when `/v1/ingest` requires a token. `None` = ingest is open, as
    /// it always was.
    ingest_tokens: Option<Arc<TokenStore>>,
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
            ingest_tokens: None,
        })
    }

    /// Require a valid token from `tokens` on every `/v1/ingest` request.
    #[must_use]
    pub fn require_ingest_token(mut self, tokens: TokenStore) -> Self {
        self.ingest_tokens = Some(Arc::new(tokens));
        self
    }

    /// The token store guarding ingest, if ingest is guarded.
    #[must_use]
    pub fn ingest_tokens(&self) -> Option<&TokenStore> {
        self.ingest_tokens.as_deref()
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }
}
