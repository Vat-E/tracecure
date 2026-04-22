use std::path::Path;

use crate::store::Store;

/// Shared application state. Cheap to clone — everything inside is `Arc`-shared.
#[derive(Clone)]
pub struct AppState {
    store: Store,
}

impl AppState {
    /// Open the app state rooted at `data_path`. Creates parent directories
    /// as needed. The data file itself is created lazily on first write.
    pub fn open(data_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = data_path.as_ref().to_path_buf();
        // We need async (tokio::fs) but callers may be sync or may already be
        // inside a single-threaded tokio runtime (e.g. `#[tokio::test]`).
        // Spinning up a fresh runtime on a dedicated thread avoids the
        // "cannot start runtime within runtime" and "block_in_place requires
        // multi-thread scheduler" errors in both cases.
        let store = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime")
                .block_on(Store::open(path))
        })
        .join()
        .expect("store-open thread panicked")?;
        Ok(Self { store })
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }
}
