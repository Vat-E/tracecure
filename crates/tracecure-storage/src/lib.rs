//! `tracecure-storage` — Tracecure's M1 storage layer: WAL, memtable, parquet segments.
//!
//! The public surface is a single `Store` type with the same four methods as
//! M0's `tracecure_server::Store` (`open`, `append`, `search`, `subscribe`).
//! See `crates/tracecure-storage/README.md` for the on-disk layout, the
//! tunable constants and the deliberate deviations from `PLAN.md` §4.2.

pub mod arrow;
pub mod compactor;
pub mod filter;
pub mod memtable;
pub mod partition;
pub mod retention;
pub mod segment;
pub mod store;
pub mod wal;

pub use compactor::{CompactionReport, CompactorConfig};
pub use filter::{NamePattern, SearchFilter};
pub use partition::PartitionConfig;
pub use retention::RetentionConfig;
pub use store::{Page, Store, StoreConfig};
