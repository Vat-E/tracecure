use std::path::Path;

use crate::store::Store;

/// Shared application state. Cheap to clone — everything inside is `Arc`-shared.
#[derive(Clone)]
pub struct AppState {
    store: Store,
}

impl AppState {
    /// Open the app state rooted at `data_path`. Creates parent directories
    /// as needed. The data file itself is created if missing.
    pub async fn open(data_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let store = Store::open(data_path).await?;
        Ok(Self { store })
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }
}
