//! `sift-storage` — Sift's M1 storage layer: WAL, memtable, parquet segments.
//!
//! The public surface is a single `Store` type with the same four methods as
//! M0's `sift_server::Store` (`open`, `append`, `search`, `subscribe`). See the
//! design spec at `docs/superpowers/specs/2026-04-23-sp1-sift-storage-design.md`.

#![allow(dead_code)] // Cleared in the last task once Store wires everything up.

pub mod arrow;
pub mod memtable;
pub mod segment;
pub mod store;
pub mod wal;

pub use store::Store;
