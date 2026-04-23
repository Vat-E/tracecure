//! WAL: serde_json-bodied framed records with a 10ms/1MB group-commit ticker.
//!
//! Frame layout (little-endian):
//!
//!   ┌──────────┬─────────────────────┬──────────┐
//!   │ len: u32 │ `serde_json(record)`  │ crc: u32 │
//!   └──────────┴─────────────────────┴──────────┘
//!         4            len                4
//!
//! Why `serde_json` and not bincode: `Record` uses `#[serde(skip_serializing_if)]`
//! on empty label/field maps and on `Option` fields. Bincode is positional
//! (no field names on the wire) so skipped-on-write fields desync the
//! decoder. JSON is self-describing, which tolerates optional fields
//! round-tripping correctly. The JSON body is ~3-4× larger than bincode
//! would be, but the WAL is bounded by memtable size (256 MB default) so
//! this is not a pressure point.
//!
//! CRC32 is computed over the `serde_json` body only (not the len prefix).

use std::io::{self, Read};

use anyhow::Context;
use sift_core::Record;

const FRAME_LEN_PREFIX: usize = 4;
const FRAME_CRC_SUFFIX: usize = 4;

pub fn encode_frame(record: &Record) -> anyhow::Result<Vec<u8>> {
    let body = serde_json::to_vec(record).context("serde_json serialize")?;
    let len: u32 = body.len().try_into().context("record too large (>4 GiB)")?;
    let crc = crc32fast::hash(&body);
    let mut out = Vec::with_capacity(FRAME_LEN_PREFIX + body.len() + FRAME_CRC_SUFFIX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc.to_le_bytes());
    Ok(out)
}

/// Outcome of trying to read one frame from `r`.
#[derive(Debug)]
pub enum FrameRead {
    Record(Record),
    /// Clean EOF: no bytes left at all. Normal at end of WAL.
    Eof,
    /// Partial or corrupt frame at the tail. Callers should stop replay here,
    /// log a warning, and NOT treat it as a fatal error.
    Partial,
}

pub fn decode_frame<R: Read>(r: &mut R) -> anyhow::Result<FrameRead> {
    let mut len_buf = [0u8; FRAME_LEN_PREFIX];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(FrameRead::Eof),
        Err(e) => return Err(e).context("read len"),
    }
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut body = vec![0u8; len];
    match r.read_exact(&mut body) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(FrameRead::Partial),
        Err(e) => return Err(e).context("read body"),
    }

    let mut crc_buf = [0u8; FRAME_CRC_SUFFIX];
    match r.read_exact(&mut crc_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(FrameRead::Partial),
        Err(e) => return Err(e).context("read crc"),
    }
    let expected = u32::from_le_bytes(crc_buf);
    let actual = crc32fast::hash(&body);
    if expected != actual {
        return Ok(FrameRead::Partial);
    }

    let record: Record = serde_json::from_slice(&body).context("serde_json deserialize")?;
    Ok(FrameRead::Record(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;
    use std::io::Cursor;

    fn sample() -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 12, 0, 0).unwrap(),
            host: "box-1".into(),
            app: "api".into(),
            stream_id: "api".into(),
            level: Level::Warn,
            body: "roundtrip me".into(),
            labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn encode_decode_roundtrips_single_frame() {
        let r = sample();
        let bytes = encode_frame(&r).unwrap();
        let mut cur = Cursor::new(&bytes);
        match decode_frame(&mut cur).unwrap() {
            FrameRead::Record(got) => assert_eq!(got, r),
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn decode_on_empty_input_is_eof() {
        let mut cur = Cursor::new(Vec::<u8>::new());
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Eof));
    }

    #[test]
    fn decode_on_truncated_body_is_partial() {
        let bytes = encode_frame(&sample()).unwrap();
        // Keep len prefix + half the body.
        let cutoff = FRAME_LEN_PREFIX + (bytes.len() - FRAME_LEN_PREFIX - FRAME_CRC_SUFFIX) / 2;
        let mut cur = Cursor::new(&bytes[..cutoff]);
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Partial));
    }

    #[test]
    fn decode_on_bad_crc_is_partial() {
        let mut bytes = encode_frame(&sample()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let mut cur = Cursor::new(&bytes);
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Partial));
    }

    #[test]
    fn multiple_frames_decode_in_order() {
        let mut buf = Vec::new();
        for i in 0..3 {
            let mut r = sample();
            r.body = format!("msg {i}");
            buf.extend(encode_frame(&r).unwrap());
        }
        let mut cur = Cursor::new(buf);
        for i in 0..3 {
            match decode_frame(&mut cur).unwrap() {
                FrameRead::Record(r) => assert_eq!(r.body, format!("msg {i}")),
                other => panic!("expected Record, got {other:?}"),
            }
        }
        assert!(matches!(decode_frame(&mut cur).unwrap(), FrameRead::Eof));
    }
}
