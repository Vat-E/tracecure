//! Browser sessions and in-flight sign-ins.
//!
//! Both are kept in memory. A session cookie carries a random id and
//! nothing else — who it belongs to lives here, so signing someone out, or
//! restarting the server, ends the session for real rather than leaving a
//! self-describing cookie that stays valid until it expires.
//!
//! The cost is that a restart signs everyone out. With a provider that
//! keeps its own session that costs one redirect, not a password prompt.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::oidc::UserInfo;

pub const SESSION_COOKIE: &str = "tc_session";
pub const LOGIN_COOKIE: &str = "tc_login";

/// How long a session lasts. Long, because the alternative is signing in
/// again every day to read logs.
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// How long a sign-in may take between leaving for the provider and coming
/// back.
pub const LOGIN_TTL: Duration = Duration::from_secs(10 * 60);

/// Caps, so that unauthenticated requests to `/auth/login` cannot grow
/// memory without bound.
const MAX_PENDING_LOGINS: usize = 10_000;
const MAX_SESSIONS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user: UserInfo,
}

/// A sign-in that has gone to the provider and not come back yet.
#[derive(Debug, Clone)]
pub struct PendingLogin {
    /// The PKCE verifier whose challenge was sent to the provider.
    pub code_verifier: String,
    /// Where to send the browser afterwards. Always a local path.
    pub next: String,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, (Session, Instant)>,
    pending: HashMap<String, (PendingLogin, Instant)>,
}

#[derive(Default)]
pub struct SessionStore {
    inner: Mutex<Inner>,
}

impl SessionStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Remember a sign-in under its `state`.
    pub fn begin_login(&self, state: String, login: PendingLogin) {
        self.begin_login_at(state, login, Instant::now());
    }

    fn begin_login_at(&self, state: String, login: PendingLogin, now: Instant) {
        let mut inner = self.lock();
        inner.pending.retain(|_, (_, expires)| *expires > now);
        if inner.pending.len() >= MAX_PENDING_LOGINS {
            // Full of sign-ins nobody finished. Drop them all rather than
            // refuse a real one; the worst case is someone clicks again.
            inner.pending.clear();
        }
        inner.pending.insert(state, (login, now + LOGIN_TTL));
    }

    /// Take a pending sign-in. It can be taken once: a `state` that has
    /// been used, has expired, or was never issued gets `None`.
    pub fn finish_login(&self, state: &str) -> Option<PendingLogin> {
        self.finish_login_at(state, Instant::now())
    }

    fn finish_login_at(&self, state: &str, now: Instant) -> Option<PendingLogin> {
        let (login, expires) = self.lock().pending.remove(state)?;
        (expires > now).then_some(login)
    }

    /// Start a session for a signed-in user.
    pub fn create(&self, id: String, user: UserInfo) {
        self.create_at(id, user, Instant::now());
    }

    fn create_at(&self, id: String, user: UserInfo, now: Instant) {
        let mut inner = self.lock();
        inner.sessions.retain(|_, (_, expires)| *expires > now);
        if inner.sessions.len() >= MAX_SESSIONS {
            inner.sessions.clear();
        }
        inner
            .sessions
            .insert(id, (Session { user }, now + SESSION_TTL));
    }

    /// The session for a cookie value, if it exists and has not expired.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Session> {
        self.get_at(id, Instant::now())
    }

    fn get_at(&self, id: &str, now: Instant) -> Option<Session> {
        let mut inner = self.lock();
        match inner.sessions.get(id) {
            Some((session, expires)) if *expires > now => Some(session.clone()),
            Some(_) => {
                inner.sessions.remove(id);
                None
            }
            None => None,
        }
    }

    /// End a session. Returns whether there was one.
    pub fn remove(&self, id: &str) -> bool {
        self.lock().sessions.remove(id).is_some()
    }
}

/// The value of one cookie from a `Cookie` request header.
#[must_use]
pub fn cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name && !v.is_empty()).then_some(v)
    })
}

/// A `Set-Cookie` value. `HttpOnly` so script cannot read it; `SameSite=Lax`
/// so it is sent on the top-level navigation back from the provider but not
/// on cross-site subrequests.
#[must_use]
pub fn set_cookie(name: &str, value: &str, max_age: Duration, secure: bool) -> String {
    format!(
        "{name}={value}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{}",
        max_age.as_secs(),
        if secure { "; Secure" } else { "" }
    )
}

/// A `Set-Cookie` value that removes a cookie.
#[must_use]
pub fn clear_cookie(name: &str, secure: bool) -> String {
    set_cookie(name, "", Duration::ZERO, secure)
}

/// Reduce a `next` parameter to something safe to redirect to: a path on
/// this server. Anything else — another site, a scheme-relative `//host`, a
/// backslash trick, the auth routes themselves — becomes `/`.
///
/// Without this, `/auth/login?next=https://evil.example` would make this
/// server a redirector that lends its name to someone else's page.
#[must_use]
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n)
            if n.starts_with('/')
                && !n.starts_with("//")
                && !n.contains('\\')
                && !n.contains(|c: char| c.is_control())
                && !n.starts_with("/auth/") =>
        {
            n.to_string()
        }
        _ => "/".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(sub: &str) -> UserInfo {
        UserInfo {
            sub: sub.into(),
            ..UserInfo::default()
        }
    }

    fn pending(next: &str) -> PendingLogin {
        PendingLogin {
            code_verifier: "v".into(),
            next: next.into(),
        }
    }

    #[test]
    fn a_session_is_found_until_it_expires() {
        let s = SessionStore::default();
        let t0 = Instant::now();
        s.create_at("id".into(), user("u1"), t0);
        assert_eq!(s.get_at("id", t0).unwrap().user.sub, "u1");
        let just_before = t0 + Duration::from_secs(SESSION_TTL.as_secs() - 1);
        assert!(s.get_at("id", just_before).is_some());
        assert!(s.get_at("id", t0 + SESSION_TTL).is_none());
        // And it is gone, not merely hidden.
        assert!(!s.remove("id"));
    }

    #[test]
    fn an_unknown_or_removed_session_is_none() {
        let s = SessionStore::default();
        assert!(s.get("nope").is_none());
        s.create("id".into(), user("u1"));
        assert!(s.remove("id"));
        assert!(s.get("id").is_none());
    }

    #[test]
    fn a_pending_login_can_be_finished_exactly_once() {
        let s = SessionStore::default();
        s.begin_login("state".into(), pending("/x"));
        assert_eq!(s.finish_login("state").unwrap().next, "/x");
        assert!(s.finish_login("state").is_none(), "replay must fail");
        assert!(s.finish_login("never-issued").is_none());
    }

    #[test]
    fn a_pending_login_expires() {
        let s = SessionStore::default();
        let t0 = Instant::now();
        s.begin_login_at("state".into(), pending("/"), t0);
        assert!(s.finish_login_at("state", t0 + LOGIN_TTL).is_none());
    }

    #[test]
    fn expired_pending_logins_are_swept_when_a_new_one_starts() {
        let s = SessionStore::default();
        let t0 = Instant::now();
        s.begin_login_at("old".into(), pending("/"), t0);
        s.begin_login_at(
            "new".into(),
            pending("/"),
            t0 + LOGIN_TTL + Duration::from_secs(1),
        );
        assert_eq!(s.lock().pending.len(), 1);
    }

    #[test]
    fn cookie_reads_one_value_out_of_the_header() {
        let h = "a=1; tc_session=abc; other=x=y";
        assert_eq!(cookie(h, "tc_session"), Some("abc"));
        assert_eq!(cookie(h, "a"), Some("1"));
        assert_eq!(cookie(h, "other"), Some("x=y"));
        assert_eq!(cookie(h, "missing"), None);
        // A name that is only a suffix of another must not match.
        assert_eq!(cookie("xtc_session=1", "tc_session"), None);
        assert_eq!(cookie("tc_session=", "tc_session"), None);
    }

    #[test]
    fn set_cookie_is_http_only_lax_and_secure_only_when_asked() {
        let c = set_cookie("tc_session", "abc", Duration::from_secs(60), true);
        assert_eq!(
            c,
            "tc_session=abc; Path=/; Max-Age=60; HttpOnly; SameSite=Lax; Secure"
        );
        assert!(!set_cookie("n", "v", Duration::from_secs(1), false).contains("Secure"));
        assert!(clear_cookie("n", false).contains("Max-Age=0"));
    }

    #[test]
    fn safe_next_keeps_local_paths() {
        assert_eq!(safe_next(Some("/")), "/");
        assert_eq!(safe_next(Some("/?q=error&w=3600")), "/?q=error&w=3600");
        assert_eq!(safe_next(Some("/some/page")), "/some/page");
    }

    #[test]
    fn safe_next_refuses_anything_that_leaves_this_server() {
        for bad in [
            "https://evil.example/",
            "//evil.example/",
            "/\\evil.example",
            "evil.example",
            "javascript:alert(1)",
            "",
            "/auth/login",
            "/auth/callback?code=x",
            "/x\ny",
        ] {
            assert_eq!(safe_next(Some(bad)), "/", "{bad:?}");
        }
        assert_eq!(safe_next(None), "/");
    }
}
