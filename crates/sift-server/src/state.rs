use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Shared application state. Cheap to clone — everything is `Arc`-shared.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    data_path: PathBuf,
}

impl AppState {
    /// Open the app state rooted at `data_path`. Creates the parent directory
    /// if it doesn't exist, but does not touch the file itself yet — that
    /// happens on first write.
    pub fn open(data_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data_path = data_path.as_ref().to_path_buf();
        if let Some(parent) = data_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        Ok(Self { inner: Arc::new(Inner { data_path }) })
    }

    #[must_use]
    pub fn data_path(&self) -> &Path {
        &self.inner.data_path
    }
}
