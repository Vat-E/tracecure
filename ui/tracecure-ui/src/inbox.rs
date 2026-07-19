//! Burst buffer between the WebSocket and `AppState`.
//!
//! Each `onmessage` is its own macrotask, so writing state per message means
//! one full reactive notification cascade per record — 500 of them a second
//! at 500 rec/s, no matter how cheap each one is. The inbox accumulates
//! arrivals and hands them over in one batch per animation frame, which
//! bounds the cascade rate at ~60/s regardless of ingest rate.
//!
//! The buffer is capped. `requestAnimationFrame` does not fire in a hidden
//! tab, so an uncapped inbox would grow for as long as a background tab sits
//! there — reintroducing exactly the unbounded growth this work is meant to
//! remove. Anything past the cap would be evicted by `AppState` on arrival
//! anyway.

use std::collections::VecDeque;

use tracecure_core::Record;

pub struct Inbox {
    buf: VecDeque<Record>,
    cap: usize,
    /// True once a flush has been scheduled and before it has run, so a
    /// burst of N records schedules one flush rather than N.
    scheduled: bool,
}

impl Inbox {
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            cap,
            scheduled: false,
        }
    }

    /// Buffer a record.
    ///
    /// Returns `true` when the caller must schedule a flush; `false` when a
    /// flush is already pending.
    pub fn push(&mut self, r: Record) -> bool {
        if self.buf.len() >= self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(r);

        if self.scheduled {
            false
        } else {
            self.scheduled = true;
            true
        }
    }

    /// Take everything buffered and re-arm scheduling.
    pub fn drain(&mut self) -> Vec<Record> {
        self.scheduled = false;
        self.buf.drain(..).collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;
    use tracecure_core::Level;

    fn rec(body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap(),
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
    fn first_push_schedules_a_flush_and_the_rest_do_not() {
        let mut ib = Inbox::new(10);
        assert!(ib.push(rec("1")), "first push must schedule");
        assert!(!ib.push(rec("2")), "a flush is already pending");
        assert!(!ib.push(rec("3")));
        assert_eq!(ib.len(), 3);
    }

    #[test]
    fn draining_rearms_scheduling() {
        let mut ib = Inbox::new(10);
        ib.push(rec("1"));
        let batch = ib.drain();
        assert_eq!(batch.len(), 1);
        assert!(ib.is_empty());
        assert!(
            ib.push(rec("2")),
            "after a flush, the next arrival must schedule the next one"
        );
    }

    #[test]
    fn drain_preserves_arrival_order() {
        let mut ib = Inbox::new(10);
        for b in ["1", "2", "3"] {
            ib.push(rec(b));
        }
        let bodies: Vec<String> = ib.drain().into_iter().map(|r| r.body).collect();
        assert_eq!(bodies, vec!["1", "2", "3"]);
    }

    /// The client used to deserialize the socket payload as a bare `Record`.
    /// Record frames survived only by accident — `TailFrame` is internally
    /// tagged and `Record` ignores the unknown `"type"` key — while every
    /// other frame was silently discarded. Most importantly that included
    /// the server's lag warning, which fires exactly when records are being
    /// dropped. These pin the envelope both ways.
    #[test]
    fn a_record_frame_round_trips_through_the_envelope() {
        let json = serde_json::to_string(&tracecure_api::TailFrame::Record(rec("hello"))).unwrap();
        assert!(
            json.contains(r#""type":"record""#),
            "frames are internally tagged: {json}"
        );
        match serde_json::from_str::<tracecure_api::TailFrame>(&json) {
            Ok(tracecure_api::TailFrame::Record(r)) => assert_eq!(r.body, "hello"),
            other => panic!("expected a record frame, got {other:?}"),
        }
    }

    #[test]
    fn an_error_frame_is_not_mistaken_for_a_record() {
        // The exact shape tracecure-server emits on broadcast lag.
        let json = r#"{"type":"error","message":"tail lagged by 12 records — reconnect"}"#;

        assert!(
            serde_json::from_str::<Record>(json).is_err(),
            "parsing frames as bare Records is what silently swallowed this"
        );
        match serde_json::from_str::<tracecure_api::TailFrame>(json) {
            Ok(tracecure_api::TailFrame::Error { message }) => {
                assert!(message.contains("lagged"), "got: {message}");
            }
            other => panic!("expected an error frame, got {other:?}"),
        }
    }

    #[test]
    fn a_heartbeat_frame_parses_and_carries_nothing() {
        // Captured from the running server.
        let json = r#"{"type":"heartbeat"}"#;
        assert!(matches!(
            serde_json::from_str::<tracecure_api::TailFrame>(json),
            Ok(tracecure_api::TailFrame::Heartbeat)
        ));
    }

    /// A hidden tab never gets an animation frame; the buffer must not grow
    /// without bound while it waits.
    #[test]
    fn buffer_is_capped_dropping_oldest() {
        let mut ib = Inbox::new(3);
        for b in ["1", "2", "3", "4", "5"] {
            ib.push(rec(b));
        }
        assert_eq!(ib.len(), 3, "must not exceed cap");
        let bodies: Vec<String> = ib.drain().into_iter().map(|r| r.body).collect();
        assert_eq!(bodies, vec!["3", "4", "5"], "newest are retained");
    }
}
