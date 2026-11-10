//! Signing people in through an external `OpenID` Connect provider.
//!
//! Tracecure keeps no accounts and no passwords. It sends the browser to the
//! provider, gets an authorization code back, exchanges it, asks the provider
//! who that was, and decides whether that person may read logs.
//!
//! The flow is authorization code with PKCE (RFC 7636). The identity comes
//! from the provider's userinfo endpoint, called with the access token the
//! code was just exchanged for, over TLS, server to server — so there is no
//! ID token to parse and no signature to verify here, and no JWT or crypto
//! dependency beyond SHA-256. See ADR-005.

use std::collections::BTreeSet;

use anyhow::Context;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// How the server is told to use a provider.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// The provider's issuer URL, e.g. `https://login.example.com`.
    pub issuer: String,
    pub client_id: String,
    /// `None` for a public client, which relies on PKCE alone.
    pub client_secret: Option<String>,
    /// Where this server is reached by browsers, e.g.
    /// `https://logs.example.com`. The callback is `{public_url}/auth/callback`.
    pub public_url: String,
    pub allow: AllowList,
}

impl OidcConfig {
    #[must_use]
    pub fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.public_url.trim_end_matches('/'))
    }

    /// Cookies are marked `Secure` when the public URL is HTTPS. Over plain
    /// HTTP (local development) a `Secure` cookie would never be sent back.
    #[must_use]
    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

/// Who may sign in, once the provider has said who they are.
///
/// Being able to authenticate at the provider is not the same as being
/// allowed to read these logs — a provider may let anyone register. So there
/// is no implicit "everyone": the operator names people, names groups, or
/// says "anyone" in so many words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowList {
    /// Lowercased e-mail addresses.
    pub emails: BTreeSet<String>,
    pub groups: BTreeSet<String>,
    pub any_user: bool,
}

impl AllowList {
    /// True if no rule was given at all — a configuration error, since it
    /// would let nobody in while looking like it was set up.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.any_user && self.emails.is_empty() && self.groups.is_empty()
    }

    /// Decide whether this person may sign in, and if not, why — the reason
    /// is shown to them, so it names what to ask the operator for.
    ///
    /// # Errors
    ///
    /// Returns the reason the user is not allowed.
    pub fn check(&self, user: &UserInfo) -> Result<(), String> {
        if self.any_user {
            return Ok(());
        }
        if user.groups.iter().any(|g| self.groups.contains(g)) {
            return Ok(());
        }
        if let Some(email) = &user.email {
            if self.emails.contains(&email.to_lowercase()) {
                // An address the provider has not verified proves nothing:
                // anyone could have typed it at registration.
                return if user.email_verified {
                    Ok(())
                } else {
                    Err(format!(
                        "{email} is on the allow list, but the identity provider has not verified that address"
                    ))
                };
            }
        }
        Err(match &user.email {
            Some(email) => format!("{email} is not allowed to use this Tracecure instance"),
            None => "this account is not allowed to use this Tracecure instance".to_string(),
        })
    }
}

/// The endpoints read from the provider's discovery document.
#[derive(Debug, Clone, Deserialize)]
pub struct Provider {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    /// Where to send the browser to end the provider's own session
    /// (RP-initiated logout). Not every provider has one.
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
}

/// The path the browser lands on once it is signed out.
pub const SIGNED_OUT_PATH: &str = "/auth/signed-out";

/// Where the browser goes after this server's session has ended.
///
/// Ending only this server's session is not a sign-out anyone can see: the
/// next page load redirects to the provider, which still knows the browser
/// and signs it straight back in. So when the provider has an end-session
/// endpoint the browser is sent there, asking to come back to the signed-out
/// page; otherwise it goes to that page directly, which at least does not
/// start a new sign-in by itself.
#[must_use]
pub fn sign_out_url(
    end_session_endpoint: Option<&str>,
    client_id: &str,
    public_url: &str,
) -> String {
    let back = format!("{}{SIGNED_OUT_PATH}", public_url.trim_end_matches('/'));
    match end_session_endpoint.map(reqwest::Url::parse) {
        Some(Ok(mut url)) => {
            url.query_pairs_mut()
                .append_pair("client_id", client_id)
                .append_pair("post_logout_redirect_uri", &back);
            url.into()
        }
        // No endpoint, or one that is not a URL: stay on this server.
        _ => SIGNED_OUT_PATH.to_string(),
    }
}

/// What the provider says about the person who signed in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct UserInfo {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub preferred_username: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
}

impl UserInfo {
    /// What to call this person on screen.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.preferred_username.as_deref())
            .or(self.email.as_deref())
            .unwrap_or(&self.sub)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// A configured provider: discovery done, ready to run the flow.
pub struct OidcClient {
    pub config: OidcConfig,
    pub provider: Provider,
    http: reqwest::Client,
}

impl OidcClient {
    /// Fetch the provider's discovery document and check it is the provider
    /// that was asked for.
    ///
    /// # Errors
    ///
    /// Fails if the document cannot be fetched or parsed, or if its `issuer`
    /// is not the configured one. That last check is what stops a typo, or a
    /// redirect, from quietly pointing sign-in at someone else's provider.
    pub async fn discover(config: OidcConfig) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            // Never follow a redirect on these calls: they carry the code,
            // the client secret and the access token.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let issuer = config.issuer.trim_end_matches('/').to_string();
        let url = format!("{issuer}/.well-known/openid-configuration");
        let provider: Provider = http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("fetching {url}"))?
            .error_for_status()
            .with_context(|| format!("fetching {url}"))?
            .json()
            .await
            .with_context(|| format!("parsing {url}"))?;
        anyhow::ensure!(
            provider.issuer.trim_end_matches('/') == issuer,
            "{url} describes issuer \"{}\", not \"{issuer}\"",
            provider.issuer
        );
        Ok(Self {
            config,
            provider,
            http,
        })
    }

    /// The URL to send the browser to.
    ///
    /// # Errors
    ///
    /// Fails only if the provider's authorization endpoint is not a URL.
    pub fn authorize_url(&self, state: &str, code_challenge: &str) -> anyhow::Result<String> {
        let mut url = reqwest::Url::parse(&self.provider.authorization_endpoint)
            .context("provider authorization_endpoint is not a URL")?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri())
            .append_pair("scope", "openid profile email groups")
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    /// Exchange the authorization code and ask the provider who it was for.
    ///
    /// # Errors
    ///
    /// Fails if either call fails or returns something unparseable. The
    /// error is for the server log; it can contain the provider's response.
    pub async fn identify(&self, code: &str, code_verifier: &str) -> anyhow::Result<UserInfo> {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.config.redirect_uri()),
            ("client_id", &self.config.client_id),
            ("code_verifier", code_verifier),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect::<Vec<_>>();
        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret", secret.clone()));
        }

        let resp = self
            .http
            .post(&self.provider.token_endpoint)
            .form(&form)
            .send()
            .await
            .context("calling the token endpoint")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("token endpoint answered {status}: {}", truncate(&body, 300));
        }
        let token: TokenResponse = resp.json().await.context("parsing the token response")?;

        let user: UserInfo = self
            .http
            .get(&self.provider.userinfo_endpoint)
            .bearer_auth(&token.access_token)
            .send()
            .await
            .context("calling the userinfo endpoint")?
            .error_for_status()
            .context("userinfo endpoint")?
            .json()
            .await
            .context("parsing the userinfo response")?;
        anyhow::ensure!(!user.sub.is_empty(), "userinfo has an empty `sub`");
        Ok(user)
    }
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// The PKCE code challenge for a verifier: `BASE64URL(SHA256(verifier))`.
#[must_use]
pub fn code_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// URL-safe base64 without padding (RFC 4648 §5), as PKCE requires.
#[must_use]
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| ALPHABET[((n >> shift) & 0x3f) as usize] as char;
        out.push(sextet(18));
        out.push(sextet(12));
        if chunk.len() > 1 {
            out.push(sextet(6));
        }
        if chunk.len() > 2 {
            out.push(sextet(0));
        }
    }
    out
}

/// A random URL-safe string with `bytes` bytes of entropy.
///
/// # Errors
///
/// Fails only if the operating system has no secure random source.
pub fn random_token(bytes: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no secure randomness: {e}"))?;
    Ok(base64url(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(email: Option<&str>, verified: bool, groups: &[&str]) -> UserInfo {
        UserInfo {
            sub: "u1".into(),
            email: email.map(str::to_string),
            email_verified: verified,
            groups: groups.iter().map(|g| (*g).to_string()).collect(),
            ..UserInfo::default()
        }
    }

    fn allow(emails: &[&str], groups: &[&str], any_user: bool) -> AllowList {
        AllowList {
            emails: emails.iter().map(|e| e.to_lowercase()).collect(),
            groups: groups.iter().map(|g| (*g).to_string()).collect(),
            any_user,
        }
    }

    /// The worked example from RFC 7636, Appendix B.
    #[test]
    fn code_challenge_matches_the_rfc_7636_vector() {
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    /// RFC 4648 §10 vectors, in the URL-safe alphabet and without padding.
    #[test]
    fn sign_out_goes_through_the_provider_when_it_can() {
        let url = sign_out_url(
            Some("https://login.example.com/oauth/logout"),
            "tracecure",
            "https://logs.example.com/",
        );
        assert_eq!(
            url,
            "https://login.example.com/oauth/logout?client_id=tracecure\
             &post_logout_redirect_uri=https%3A%2F%2Flogs.example.com%2Fauth%2Fsigned-out"
        );
        // Without an endpoint — or with a broken one — it stays local.
        assert_eq!(sign_out_url(None, "c", "https://x"), "/auth/signed-out");
        assert_eq!(
            sign_out_url(Some("not a url"), "c", "https://x"),
            "/auth/signed-out"
        );
    }

    #[test]
    fn base64url_matches_the_rfc_4648_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        // The two characters that differ from standard base64.
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn random_tokens_are_url_safe_and_differ() {
        let a = random_token(32).unwrap();
        let b = random_token(32).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
    }

    #[test]
    fn an_empty_allow_list_is_detectable() {
        assert!(AllowList::default().is_empty());
        assert!(!allow(&[], &[], true).is_empty());
        assert!(!allow(&["a@x"], &[], false).is_empty());
        assert!(!allow(&[], &["ops"], false).is_empty());
    }

    #[test]
    fn a_listed_verified_email_is_allowed_case_insensitively() {
        let list = allow(&["Owner@Example.com"], &[], false);
        assert!(list
            .check(&user(Some("owner@example.COM"), true, &[]))
            .is_ok());
    }

    #[test]
    fn a_listed_but_unverified_email_is_refused() {
        let list = allow(&["owner@example.com"], &[], false);
        let e = list
            .check(&user(Some("owner@example.com"), false, &[]))
            .unwrap_err();
        assert!(e.contains("not verified"), "{e}");
    }

    #[test]
    fn group_membership_is_enough_whatever_the_email() {
        let list = allow(&[], &["logs"], false);
        assert!(list.check(&user(None, false, &["staff", "logs"])).is_ok());
        assert!(list.check(&user(Some("x@y"), true, &["staff"])).is_err());
    }

    #[test]
    fn an_unlisted_user_is_refused_and_told_who_they_are() {
        let list = allow(&["owner@example.com"], &["logs"], false);
        let e = list
            .check(&user(Some("stranger@example.com"), true, &["staff"]))
            .unwrap_err();
        assert!(e.contains("stranger@example.com"), "{e}");
        assert!(list.check(&user(None, false, &[])).is_err());
    }

    #[test]
    fn any_user_allows_everyone_the_provider_authenticates() {
        let list = allow(&[], &[], true);
        assert!(list.check(&user(None, false, &[])).is_ok());
    }

    #[test]
    fn display_name_falls_back_sensibly() {
        let mut u = user(Some("a@b"), true, &[]);
        assert_eq!(u.display_name(), "a@b");
        u.preferred_username = Some("alex".into());
        assert_eq!(u.display_name(), "alex");
        u.name = Some("Alex A".into());
        assert_eq!(u.display_name(), "Alex A");
        assert_eq!(user(None, false, &[]).display_name(), "u1");
    }

    #[test]
    fn redirect_uri_and_cookie_security_follow_the_public_url() {
        let cfg = |public: &str| OidcConfig {
            issuer: "https://login.example.com".into(),
            client_id: "tracecure".into(),
            client_secret: None,
            public_url: public.into(),
            allow: AllowList::default(),
        };
        assert_eq!(
            cfg("https://logs.example.com/").redirect_uri(),
            "https://logs.example.com/auth/callback"
        );
        assert!(cfg("https://logs.example.com").secure_cookies());
        assert!(!cfg("http://127.0.0.1:7789").secure_cookies());
    }

    #[test]
    fn userinfo_tolerates_missing_optional_claims() {
        let u: UserInfo = serde_json::from_str(r#"{"sub":"abc"}"#).unwrap();
        assert_eq!(u.sub, "abc");
        assert!(u.email.is_none() && !u.email_verified && u.groups.is_empty());
    }
}
