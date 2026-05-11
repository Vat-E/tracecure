//! `tracecure-storage` — Tracecure's M1 storage layer: WAL, memtable, parquet segments.
//!
//! The public surface is a single `Store` type with the same four methods as
//! M0's `tracecure_server::Store` (`open`, `append`, `search`, `subscribe`). See the
//! design spec at `docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md`.

pub mod arrow;
pub mod compactor;
pub mod memtable;
pub mod partition;
pub mod segment;
pub mod store;
pub mod wal;

pub use compactor::CompactorConfig;
pub use partition::PartitionConfig;
pub use store::{Store, StoreConfig};
