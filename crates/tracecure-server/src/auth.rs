//! Ingest tokens: who may write to `/v1/ingest`.
//!
//! A token looks like `tc_<id>_<secret>`. The `id` is public — it is what
//! `token list` shows and what `token revoke` takes. The `secret` is 32
//! random bytes and is shown exactly once, when the token is created. Only
//! its SHA-256 is stored, so reading the token file does not let anyone
//! write logs.
//!
//! A fast hash is the right tool here, unlike for passwords: the secret is
//! 256 bits of randomness, so there is nothing to brute-force that a slow
//! hash would protect.
//!
//! Tokens live in `{data}/tokens.json`. The server re-reads the file when it
//! changes, so creating or revoking a token with the CLI takes effect on the
//! running server without a restart. See ADR-004.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TOKEN_PREFIX: &str = "tc_";
const FILE_NAME: &str = "tokens.json";
const FILE_VERSION: u32 = 1;
const ID_BYTES: usize = 4;
const SECRET_BYTES: usize = 32;
const MAX_NAME_LEN: usize = 64;

/// The only scope there is so far. Stored so that a later read scope does
/// not have to guess what existing tokens were allowed to do.
pub const SCOPE_INGEST: &str = "ingest";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRecord {
    pub id: String,
    /// What the token is for — a project or a host. Unique.
    pub name: String,
    pub scope: String,
    /// Lowercase hex SHA-256 of the secret. Never the secret.
    pub hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TokenFile {
    version: u32,
    tokens: Vec<TokenRecord>,
}

#[derive(Default)]
struct Cache {
    /// What the file looked like when `tokens` was read: its mtime and
    /// length, or `None` if it did not exist.
    stamp: Option<(SystemTime, u64)>,
    loaded: bool,
    tokens: Vec<TokenRecord>,
}

pub struct TokenStore {
    path: PathBuf,
    cache: Mutex<Cache>,
}

impl TokenStore {
    /// The token store for a data directory. Does not touch the disk; a
    /// missing file is simply "no tokens".
    #[must_use]
    pub fn open(data_dir: impl AsRef<Path>) -> Self {
        Self {
            path: data_dir.as_ref().join(FILE_NAME),
            cache: Mutex::new(Cache::default()),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create a token. Returns its record and the full token string — the
    /// only time the secret exists outside the caller's hands.
    ///
    /// # Errors
    ///
    /// Fails if the name is empty, too long, has control characters or is
    /// already taken, or if the file cannot be read or written.
    pub fn create(&self, name: &str) -> anyhow::Result<(TokenRecord, String)> {
        let name = validate_name(name)?;
        let mut tokens = self.read_file()?;
        anyhow::ensure!(
            !tokens.iter().any(|t| t.name == name),
            "a token named \"{name}\" already exists; revoke it first or pick another name"
        );

        // Ids are four random bytes. A collision is unlikely and harmless to
        // retry, but must not be allowed: revoke takes an id.
        let id = loop {
            let candidate = hex(&random_bytes::<ID_BYTES>()?);
            if !tokens.iter().any(|t| t.id == candidate) {
                break candidate;
            }
        };
        let secret = hex(&random_bytes::<SECRET_BYTES>()?);
        let record = TokenRecord {
            id: id.clone(),
            name,
            scope: SCOPE_INGEST.to_string(),
            hash: hash_secret(&secret),
            created_at: Utc::now(),
        };
        tokens.push(record.clone());
        self.write_file(&tokens)?;
        Ok((record, format!("{TOKEN_PREFIX}{id}_{secret}")))
    }

    /// Every token, oldest first.
    ///
    /// # Errors
    ///
    /// Fails if the file exists but cannot be read or parsed.
    pub fn list(&self) -> anyhow::Result<Vec<TokenRecord>> {
        self.read_file()
    }

    /// Remove a token by id. Returns whether there was one.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read or written.
    pub fn revoke(&self, id: &str) -> anyhow::Result<bool> {
        let mut tokens = self.read_file()?;
        let before = tokens.len();
        tokens.retain(|t| t.id != id);
        if tokens.len() == before {
            return Ok(false);
        }
        self.write_file(&tokens)?;
        Ok(true)
    }

    /// Check a presented token. Returns its record if it is valid.
    ///
    /// Re-reads the file first if it has changed on disk. If the file is
    /// there but unreadable, every token is rejected: failing closed is the
    /// only safe answer for a check that guards a public endpoint.
    #[must_use]
    pub fn verify(&self, presented: &str) -> Option<TokenRecord> {
        let (id, secret) = split_token(presented)?;
        let presented_hash = hash_secret(secret);

        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stamp = file_stamp(&self.path);
        if !cache.loaded || cache.stamp != stamp {
            cache.tokens = match self.read_file() {
                Ok(tokens) => tokens,
                Err(e) => {
                    tracing::error!(%e, "token file unreadable; rejecting all tokens");
                    Vec::new()
                }
            };
            cache.stamp = stamp;
            cache.loaded = true;
        }

        cache
            .tokens
            .iter()
            .find(|t| t.id == id)
            .filter(|t| constant_time_eq(t.hash.as_bytes(), presented_hash.as_bytes()))
            .cloned()
    }

    fn read_file(&self) -> anyhow::Result<Vec<TokenRecord>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let file: TokenFile = serde_json::from_str(&text)
                    .with_context(|| format!("parsing {}", self.path.display()))?;
                anyhow::ensure!(
                    file.version == FILE_VERSION,
                    "{} has version {}, this server understands {FILE_VERSION}",
                    self.path.display(),
                    file.version
                );
                Ok(file.tokens)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.path.display())),
        }
    }

    /// Write the file by renaming a finished temporary over it, so a reader
    /// — the running server — never sees a half-written list and takes it
    /// for "no tokens".
    fn write_file(&self, tokens: &[TokenRecord]) -> anyhow::Result<()> {
        let file = TokenFile {
            version: FILE_VERSION,
            tokens: tokens.to_vec(),
        };
        let json = serde_json::to_string_pretty(&file)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        write_private(&tmp, json.as_bytes())
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))?;
        Ok(())
    }
}

/// Pull the bearer token out of an `Authorization` header value.
#[must_use]
pub fn bearer(header: &str) -> Option<&str> {
    let (scheme, token) = header.trim().split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}

fn split_token(presented: &str) -> Option<(&str, &str)> {
    let rest = presented.strip_prefix(TOKEN_PREFIX)?;
    let (id, secret) = rest.split_once('_')?;
    let is_hex = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit());
    (is_hex(id, ID_BYTES * 2) && is_hex(secret, SECRET_BYTES * 2)).then_some((id, secret))
}

fn validate_name(name: &str) -> anyhow::Result<String> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "a token needs a name");
    anyhow::ensure!(
        name.chars().count() <= MAX_NAME_LEN,
        "token name is longer than {MAX_NAME_LEN} characters"
    );
    anyhow::ensure!(
        !name.chars().any(char::is_control),
        "token name contains a control character"
    );
    Ok(name.to_string())
}

fn hash_secret(secret: &str) -> String {
    hex(&Sha256::digest(secret.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn random_bytes<const N: usize>() -> anyhow::Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no secure randomness: {e}"))?;
    Ok(buf)
}

/// Compare without stopping at the first difference, so how long a wrong
/// token takes to reject says nothing about how much of it was right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Create (or truncate) a file readable only by its owner where the
/// platform has that notion.
fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, TokenStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = TokenStore::open(tmp.path());
        (tmp, store)
    }

    #[test]
    fn a_missing_file_is_no_tokens_not_an_error() {
        let (_tmp, s) = store();
        assert!(s.list().unwrap().is_empty());
        assert!(s.verify("tc_00000000_x").is_none());
    }

    #[test]
    fn a_created_token_verifies_and_has_the_documented_shape() {
        let (_tmp, s) = store();
        let (record, token) = s.create("billing").unwrap();
        assert!(token.starts_with("tc_"));
        assert_eq!(token.len(), 3 + 8 + 1 + 64);
        assert_eq!(record.scope, SCOPE_INGEST);
        assert_eq!(s.verify(&token).map(|t| t.name).as_deref(), Some("billing"));
    }

    #[test]
    fn the_secret_is_not_in_the_file() {
        let (_tmp, s) = store();
        let (_, token) = s.create("billing").unwrap();
        let secret = token.rsplit('_').next().unwrap();
        let on_disk = std::fs::read_to_string(s.path()).unwrap();
        assert!(!on_disk.contains(secret));
        assert!(!on_disk.contains(&token));
    }

    #[test]
    fn a_wrong_secret_a_wrong_id_and_garbage_are_all_rejected() {
        let (_tmp, s) = store();
        let (record, token) = s.create("billing").unwrap();
        let (_, other) = s.create("web").unwrap();

        let secret = token.rsplit('_').next().unwrap();
        let other_secret = other.rsplit('_').next().unwrap();
        // Right id, another token's secret.
        assert!(s
            .verify(&format!("tc_{}_{other_secret}", record.id))
            .is_none());
        // Right secret, unknown id.
        assert!(s.verify(&format!("tc_ffffffff_{secret}")).is_none());
        for bad in [
            "",
            "tc_",
            "tc__",
            "nope",
            &token[3..],
            &token[..token.len() - 1],
        ] {
            assert!(s.verify(bad).is_none(), "{bad:?} must not verify");
        }
    }

    #[test]
    fn a_revoked_token_stops_working_and_the_others_do_not() {
        let (_tmp, s) = store();
        let (a, token_a) = s.create("a").unwrap();
        let (_, token_b) = s.create("b").unwrap();
        assert!(s.verify(&token_a).is_some());

        assert!(s.revoke(&a.id).unwrap());
        assert!(s.verify(&token_a).is_none());
        assert!(s.verify(&token_b).is_some());
        assert!(!s.revoke(&a.id).unwrap(), "already gone");
    }

    /// The CLI is a separate process from the server: it changes the file,
    /// and the server's store has to notice.
    #[test]
    fn a_change_made_through_another_store_is_picked_up() {
        let tmp = tempfile::tempdir().unwrap();
        let server = TokenStore::open(tmp.path());
        let cli = TokenStore::open(tmp.path());

        assert!(server.verify("tc_00000000_x").is_none());
        let (record, token) = cli.create("late").unwrap();
        assert!(server.verify(&token).is_some(), "created while running");
        cli.revoke(&record.id).unwrap();
        assert!(server.verify(&token).is_none(), "revoked while running");
    }

    #[test]
    fn names_must_be_unique_and_sane() {
        let (_tmp, s) = store();
        s.create("billing").unwrap();
        assert!(s.create("billing").is_err());
        assert!(s.create("  billing  ").is_err(), "trimmed before comparing");
        assert!(s.create("").is_err());
        assert!(s.create("   ").is_err());
        assert!(s.create(&"x".repeat(65)).is_err());
        assert!(s.create("bad\nname").is_err());
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn an_unreadable_token_file_rejects_everything() {
        let (_tmp, s) = store();
        let (_, token) = s.create("billing").unwrap();
        assert!(s.verify(&token).is_some());
        std::fs::write(s.path(), "{ not json").unwrap();
        assert!(s.verify(&token).is_none());
        assert!(s.list().is_err());
    }

    #[test]
    fn a_file_from_a_newer_version_is_refused() {
        let (_tmp, s) = store();
        std::fs::write(s.path(), r#"{"version":2,"tokens":[]}"#).unwrap();
        assert!(s.list().is_err());
    }

    #[test]
    fn two_tokens_never_share_a_secret_or_an_id() {
        let (_tmp, s) = store();
        let (a, ta) = s.create("a").unwrap();
        let (b, tb) = s.create("b").unwrap();
        assert_ne!(a.id, b.id);
        assert_ne!(ta, tb);
        assert_ne!(a.hash, b.hash);
    }

    #[test]
    fn bearer_reads_the_authorization_header() {
        assert_eq!(bearer("Bearer abc"), Some("abc"));
        assert_eq!(bearer("bearer   abc  "), Some("abc"));
        assert_eq!(bearer("BEARER abc"), Some("abc"));
        assert_eq!(bearer("Basic abc"), None);
        assert_eq!(bearer("Bearer "), None);
        assert_eq!(bearer("abc"), None);
    }

    #[test]
    fn constant_time_eq_is_still_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }

    #[cfg(unix)]
    #[test]
    fn the_token_file_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt as _;
        let (_tmp, s) = store();
        s.create("billing").unwrap();
        let mode = std::fs::metadata(s.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
