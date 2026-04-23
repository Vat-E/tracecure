//! In-memory Vec<Record> buffer with 256MB / 60s flush triggers.

use std::time::{Duration, Instant};

use sift_core::Record;

pub const DEFAULT_SIZE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_AGE_SECONDS: u64 = 60;

#[derive(Debug, Clone, Copy)]
pub struct MemtableConfig {
    pub size_bytes: usize,
    pub age: Duration,
}

impl Default for MemtableConfig {
    fn default() -> Self {
        Self {
            size_bytes: DEFAULT_SIZE_BYTES,
            age: Duration::from_secs(DEFAULT_AGE_SECONDS),
        }
    }
}

#[derive(Debug)]
pub struct Memtable {
    records: Vec<Record>,
    approx_bytes: usize,
    opened_at: Instant,
    cfg: MemtableConfig,
}

impl Memtable {
    #[must_use]
    pub fn with_config(cfg: MemtableConfig) -> Self {
        Self {
            records: Vec::new(),
            approx_bytes: 0,
            opened_at: Instant::now(),
            cfg,
        }
    }

    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MemtableConfig::default())
    }

    /// Normal path: push a record and update byte count.
    pub fn push(&mut self, r: Record) {
        self.approx_bytes += approx_bytes(&r);
        self.records.push(r);
    }

    /// Replay path (used during `Store::open`): push without ever triggering a
    /// flush. Callers run `should_flush` once at end of replay.
    pub fn push_replay(&mut self, r: Record) {
        self.push(r);
    }

    #[must_use]
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        self.approx_bytes
    }

    #[must_use]
    pub fn should_flush(&self) -> bool {
        self.approx_bytes >= self.cfg.size_bytes || self.opened_at.elapsed() >= self.cfg.age
    }

    /// Swap contents into a fresh `Vec` and reset the age clock.
    pub fn take(&mut self) -> Vec<Record> {
        self.approx_bytes = 0;
        self.opened_at = Instant::now();
        std::mem::take(&mut self.records)
    }
}

impl Default for Memtable {
    fn default() -> Self {
        Self::new()
    }
}

fn approx_bytes(r: &Record) -> usize {
    let label_bytes: usize = r.labels.iter().map(|(k, v)| k.len() + v.len()).sum();
    let field_bytes: usize = r.fields.iter().map(|(k, v)| k.len() + v.to_string().len()).sum();
    r.host.len() + r.app.len() + r.stream_id.len() + r.body.len() + label_bytes + field_bytes + 64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;

    fn rec(body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: body.into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn new_memtable_is_empty() {
        let m = Memtable::new();
        assert!(m.is_empty());
        assert_eq!(m.approx_bytes(), 0);
    }

    #[test]
    fn push_grows_len_and_bytes() {
        let mut m = Memtable::new();
        m.push(rec("alpha"));
        m.push(rec("beta"));
        assert_eq!(m.len(), 2);
        assert!(m.approx_bytes() > 0);
    }

    #[test]
    fn take_clears_records_and_bytes() {
        let mut m = Memtable::new();
        m.push(rec("x"));
        let taken = m.take();
        assert_eq!(taken.len(), 1);
        assert!(m.is_empty());
        assert_eq!(m.approx_bytes(), 0);
    }

    #[test]
    fn should_flush_fires_on_byte_trigger() {
        let cfg = MemtableConfig {
            size_bytes: 100,
            age: Duration::from_secs(3600),
        };
        let mut m = Memtable::with_config(cfg);
        for _ in 0..20 {
            m.push(rec("some body content so bytes rise fast"));
        }
        assert!(m.should_flush());
    }

    #[test]
    fn should_flush_fires_on_age_trigger() {
        let cfg = MemtableConfig {
            size_bytes: usize::MAX,
            age: Duration::from_millis(1),
        };
        let m = Memtable::with_config(cfg);
        std::thread::sleep(Duration::from_millis(5));
        assert!(m.should_flush());
    }

    #[test]
    fn should_flush_false_when_neither_trigger_hits() {
        let cfg = MemtableConfig {
            size_bytes: usize::MAX,
            age: Duration::from_secs(3600),
        };
        let mut m = Memtable::with_config(cfg);
        m.push(rec("x"));
        assert!(!m.should_flush());
    }
}
