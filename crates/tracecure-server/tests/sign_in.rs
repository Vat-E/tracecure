//! Sign-in end to end, against a stand-in `OpenID` Connect provider running
//! on a local port: discovery, the redirect out, the callback, the code
//! exchange with PKCE, userinfo, the allow list, the session cookie, and
//! what is and is not reachable without one.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;
use tracecure_server::oidc::{code_challenge, AllowList, OidcClient, OidcConfig};

const PUBLIC_URL: &str = "https://logs.example.test";
const CLIENT_ID: &str = "tracecure";

// ---------------------------------------------------------------- provider

/// What the stand-in provider will hand out for one authorization code.
#[derive(Clone)]
struct Grant {
    challenge: String,
    user: Value,
}

#[derive(Clone, Default)]
struct Provider {
    base: Arc<Mutex<String>>,
    grants: Arc<Mutex<HashMap<String, Grant>>>,
}

async fn discovery(State(p): State<Provider>) -> Json<Value> {
    let base = p.base.lock().unwrap().clone();
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "userinfo_endpoint": format!("{base}/oauth/userinfo"),
    }))
}

/// The token endpoint, enforcing what a real provider enforces: the code is
/// known and unused, the client and redirect match, and the verifier hashes
/// to the challenge sent at the start.
async fn token(
    State(p): State<Provider>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    let field = |k: &str| form.get(k).map(String::as_str).unwrap_or_default();
    if field("grant_type") != "authorization_code"
        || field("client_id") != CLIENT_ID
        || field("redirect_uri") != format!("{PUBLIC_URL}/auth/callback")
    {
        return Err((StatusCode::BAD_REQUEST, "invalid_request"));
    }
    let code = field("code").to_string();
    let grant = p.grants.lock().unwrap().get(&code).cloned();
    let Some(grant) = grant else {
        return Err((StatusCode::BAD_REQUEST, "invalid_grant"));
    };
    if code_challenge(field("code_verifier")) != grant.challenge {
        return Err((StatusCode::BAD_REQUEST, "invalid_grant: pkce"));
    }
    Ok(Json(
        json!({ "access_token": format!("at-{code}"), "token_type": "Bearer" }),
    ))
}

async fn userinfo(
    State(p): State<Provider>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let code = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer at-"))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let grant = p.grants.lock().unwrap().get(code).cloned();
    grant.map(|g| Json(g.user)).ok_or(StatusCode::UNAUTHORIZED)
}

async fn start_provider() -> Provider {
    let provider = Provider::default();
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/oauth/token", post(token))
        .route("/oauth/userinfo", get(userinfo))
        .with_state(provider.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    *provider.base.lock().unwrap() = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    provider
}

// ------------------------------------------------------------------- server

struct Fixture {
    _tmp: tempfile::TempDir,
    app: Router,
    provider: Provider,
}

fn emails(list: &[&str]) -> AllowList {
    AllowList {
        emails: list.iter().map(|e| e.to_lowercase()).collect(),
        ..AllowList::default()
    }
}

async fn fixture(allow: AllowList) -> Fixture {
    let provider = start_provider().await;
    let issuer = provider.base.lock().unwrap().clone();
    let oidc = OidcClient::discover(OidcConfig {
        issuer,
        client_id: CLIENT_ID.into(),
        client_secret: None,
        public_url: PUBLIC_URL.into(),
        allow,
    })
    .await
    .unwrap();

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("index.html"), "<html>ui</html>").unwrap();
    let state = tracecure_server::AppState::open(tmp.path().join("data"))
        .await
        .unwrap()
        .require_sign_in(oidc);
    let app = tracecure_server::router_with_ui(state, Some(tmp.path().to_path_buf()));
    Fixture {
        _tmp: tmp,
        app,
        provider,
    }
}

struct Reply {
    status: StatusCode,
    location: Option<String>,
    set_cookies: Vec<String>,
    body: String,
}

impl Reply {
    /// The value a named cookie was set to, if it was set to something.
    fn cookie(&self, name: &str) -> Option<String> {
        self.set_cookies.iter().find_map(|c| {
            let first = c.split(';').next()?;
            let (k, v) = first.split_once('=')?;
            (k == name && !v.is_empty()).then(|| v.to_string())
        })
    }
}

async fn send(app: &Router, method: &str, uri: &str, headers: &[(&str, &str)]) -> Reply {
    let mut req = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let location = res
        .headers()
        .get("location")
        .map(|v| v.to_str().unwrap().to_string());
    let set_cookies = res
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    Reply {
        status,
        location,
        set_cookies,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn query(url: &str) -> HashMap<String, String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// A sign-in that has left for the provider.
struct Started {
    state: String,
    challenge: String,
    login_cookie: String,
}

async fn start_login(f: &Fixture, next: Option<&str>) -> Started {
    let uri = match next {
        Some(n) => format!(
            "/auth/login?next={}",
            reqwest::Url::parse_with_params("http://x/", [("n", n)])
                .unwrap()
                .query()
                .unwrap()
                .trim_start_matches("n=")
        ),
        None => "/auth/login".to_string(),
    };
    let r = send(&f.app, "GET", &uri, &[]).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let params = query(r.location.as_deref().unwrap());
    Started {
        state: params["state"].clone(),
        challenge: params["code_challenge"].clone(),
        login_cookie: r.cookie("tc_login").expect("login cookie"),
    }
}

/// The provider signs `user` in and sends the browser back; returns the
/// server's reply to that callback.
async fn finish_login(f: &Fixture, started: &Started, user: Value) -> Reply {
    let code = format!("code-{}", started.state);
    f.provider.grants.lock().unwrap().insert(
        code.clone(),
        Grant {
            challenge: started.challenge.clone(),
            user,
        },
    );
    send(
        &f.app,
        "GET",
        &format!("/auth/callback?code={code}&state={}", started.state),
        &[("cookie", &format!("tc_login={}", started.login_cookie))],
    )
    .await
}

fn owner() -> Value {
    json!({ "sub": "u-1", "email": "owner@example.com", "email_verified": true, "name": "Owner" })
}

/// Sign in as an allowed user and return the session cookie header value.
async fn signed_in(f: &Fixture) -> String {
    let started = start_login(f, None).await;
    let r = finish_login(f, &started, owner()).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    format!(
        "tc_session={}",
        r.cookie("tc_session").expect("session cookie")
    )
}

// -------------------------------------------------------------------- tests

#[tokio::test]
async fn without_a_session_reading_is_refused_and_pages_go_to_sign_in() {
    let f = fixture(emails(&["owner@example.com"])).await;

    // An API call: 401, not a redirect to an HTML page `fetch` cannot use.
    let api = send(&f.app, "GET", "/v1/search?limit=1", &[]).await;
    assert_eq!(api.status, StatusCode::UNAUTHORIZED);
    assert!(api.body.contains("/auth/login"), "{}", api.body);

    // The live tail too — and before any WebSocket upgrade is attempted.
    let tail = send(
        &f.app,
        "GET",
        "/v1/tail",
        &[("connection", "upgrade"), ("upgrade", "websocket")],
    )
    .await;
    assert_eq!(tail.status, StatusCode::UNAUTHORIZED);

    // A page load: sent to sign-in, with the way back.
    let page = send(&f.app, "GET", "/?q=error", &[("accept", "text/html")]).await;
    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert_eq!(
        page.location.as_deref(),
        Some("/auth/login?next=%2F%3Fq%3Derror")
    );

    // The UI's own assets are not handed out either.
    let asset = send(&f.app, "GET", "/index.html", &[]).await;
    assert_eq!(asset.status, StatusCode::UNAUTHORIZED);
    assert!(
        !asset.body.contains("<html>"),
        "the page itself must not leak"
    );
}

#[tokio::test]
async fn health_and_ingest_do_not_need_a_session() {
    let f = fixture(emails(&["owner@example.com"])).await;
    assert_eq!(
        send(&f.app, "GET", "/health", &[]).await.status,
        StatusCode::OK
    );
    // Ingest is guarded by tokens, which this fixture leaves off.
    assert_eq!(
        send(&f.app, "POST", "/v1/ingest", &[]).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn login_sends_the_browser_to_the_provider_with_pkce() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let r = send(&f.app, "GET", "/auth/login", &[]).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);

    let location = r.location.clone().unwrap();
    let base = f.provider.base.lock().unwrap().clone();
    assert!(
        location.starts_with(&format!("{base}/oauth/authorize?")),
        "{location}"
    );
    let p = query(&location);
    assert_eq!(p["response_type"], "code");
    assert_eq!(p["client_id"], CLIENT_ID);
    assert_eq!(p["redirect_uri"], format!("{PUBLIC_URL}/auth/callback"));
    assert_eq!(p["code_challenge_method"], "S256");
    assert!(p["scope"].split(' ').any(|s| s == "openid"));
    assert_eq!(p["code_challenge"].len(), 43);

    let cookie = r
        .set_cookies
        .iter()
        .find(|c| c.starts_with("tc_login="))
        .unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
    assert!(
        cookie.contains("Secure"),
        "the public URL is https: {cookie}"
    );
    assert_eq!(r.cookie("tc_login").unwrap(), p["state"]);
}

#[tokio::test]
async fn a_full_sign_in_opens_reading_and_the_ui() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let started = start_login(&f, Some("/?q=timeout")).await;
    let r = finish_login(&f, &started, owner()).await;

    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert_eq!(
        r.location.as_deref(),
        Some("/?q=timeout"),
        "back where it started"
    );
    let session = r
        .set_cookies
        .iter()
        .find(|c| c.starts_with("tc_session="))
        .unwrap();
    assert!(session.contains("HttpOnly") && session.contains("Secure"));
    assert!(session.contains("Max-Age=2592000"), "30 days: {session}");
    // The one-time login cookie is cleared.
    assert!(r.set_cookies.iter().any(|c| c.starts_with("tc_login=;")));

    let cookie = format!("tc_session={}", r.cookie("tc_session").unwrap());
    let search = send(&f.app, "GET", "/v1/search?limit=1", &[("cookie", &cookie)]).await;
    assert_eq!(search.status, StatusCode::OK);
    let page = send(
        &f.app,
        "GET",
        "/",
        &[("cookie", &cookie), ("accept", "text/html")],
    )
    .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("ui"));

    let me = send(&f.app, "GET", "/auth/me", &[("cookie", &cookie)]).await;
    assert_eq!(me.status, StatusCode::OK);
    let me: Value = serde_json::from_str(&me.body).unwrap();
    assert_eq!(me["name"], "Owner");
    assert_eq!(me["email"], "owner@example.com");
}

#[tokio::test]
async fn me_without_a_session_is_a_401_that_says_where_to_sign_in() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let me = send(&f.app, "GET", "/auth/me", &[]).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
    assert!(me.body.contains("/auth/login"));
}

#[tokio::test]
async fn someone_not_on_the_allow_list_gets_no_session() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let started = start_login(&f, None).await;
    let stranger = json!({ "sub": "u-2", "email": "stranger@example.com", "email_verified": true });
    let r = finish_login(&f, &started, stranger).await;

    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.body.contains("stranger@example.com"), "{}", r.body);
    assert!(r.cookie("tc_session").is_none());
}

#[tokio::test]
async fn an_allowed_address_the_provider_has_not_verified_is_refused() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let started = start_login(&f, None).await;
    let unverified = json!({ "sub": "u-3", "email": "owner@example.com", "email_verified": false });
    let r = finish_login(&f, &started, unverified).await;

    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.cookie("tc_session").is_none());
}

#[tokio::test]
async fn group_membership_lets_someone_in() {
    let allow = AllowList {
        groups: BTreeSet::from(["logs".to_string()]),
        ..AllowList::default()
    };
    let f = fixture(allow).await;
    let started = start_login(&f, None).await;
    let member = json!({ "sub": "u-4", "groups": ["staff", "logs"] });
    let r = finish_login(&f, &started, member).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    assert!(r.cookie("tc_session").is_some());
}

/// A page shown to a refused user contains their address from the provider.
/// It must be escaped.
#[tokio::test]
async fn the_refusal_page_escapes_what_the_provider_sent() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let started = start_login(&f, None).await;
    let hostile =
        json!({ "sub": "u-5", "email": "<script>alert(1)</script>@x", "email_verified": true });
    let r = finish_login(&f, &started, hostile).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(!r.body.contains("<script>"), "{}", r.body);
    assert!(r.body.contains("&lt;script&gt;"));
}

#[tokio::test]
async fn a_callback_must_match_the_browser_that_started_it() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let started = start_login(&f, None).await;
    let code = format!("code-{}", started.state);
    f.provider.grants.lock().unwrap().insert(
        code.clone(),
        Grant {
            challenge: started.challenge.clone(),
            user: owner(),
        },
    );
    let uri = format!("/auth/callback?code={code}&state={}", started.state);

    // No login cookie at all — a link opened in another browser.
    let none = send(&f.app, "GET", &uri, &[]).await;
    assert_eq!(none.status, StatusCode::BAD_REQUEST);
    assert!(none.cookie("tc_session").is_none());

    // A login cookie from a different sign-in.
    let other = send(
        &f.app,
        "GET",
        &uri,
        &[("cookie", "tc_login=someone-elses-state")],
    )
    .await;
    assert_eq!(other.status, StatusCode::BAD_REQUEST);
    assert!(other.cookie("tc_session").is_none());
}

#[tokio::test]
async fn a_callback_cannot_be_replayed() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let started = start_login(&f, None).await;
    assert_eq!(
        finish_login(&f, &started, owner()).await.status,
        StatusCode::SEE_OTHER
    );
    let again = finish_login(&f, &started, owner()).await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    assert!(again.cookie("tc_session").is_none());
}

#[tokio::test]
async fn a_state_this_server_never_issued_is_refused() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let r = send(
        &f.app,
        "GET",
        "/auth/callback?code=x&state=made-up",
        &[("cookie", "tc_login=made-up")],
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

/// The provider checks the PKCE verifier. If the code was issued for a
/// different challenge — a code stolen from another sign-in — the exchange
/// fails and there is no session.
#[tokio::test]
async fn a_code_issued_for_another_challenge_is_useless() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let started = start_login(&f, None).await;
    let stolen = Started {
        challenge: code_challenge("some-other-verifier"),
        state: started.state.clone(),
        login_cookie: started.login_cookie.clone(),
    };
    let r = finish_login(&f, &stolen, owner()).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.cookie("tc_session").is_none());
}

#[tokio::test]
async fn an_error_from_the_provider_ends_without_a_session() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let r = send(
        &f.app,
        "GET",
        "/auth/callback?error=access_denied&error_description=You%20said%20no",
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.body.contains("You said no"));
    assert!(r.cookie("tc_session").is_none());
}

/// `next` comes from the URL, so it is attacker-controlled. It may only
/// ever lead back into this server.
#[tokio::test]
async fn next_cannot_send_the_browser_to_another_site() {
    let f = fixture(emails(&["owner@example.com"])).await;
    for evil in ["https://evil.example/", "//evil.example/", "/auth/login"] {
        let started = start_login(&f, Some(evil)).await;
        let r = finish_login(&f, &started, owner()).await;
        assert_eq!(r.location.as_deref(), Some("/"), "{evil}");
    }
}

#[tokio::test]
async fn logout_ends_the_session() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let cookie = signed_in(&f).await;
    assert_eq!(
        send(&f.app, "GET", "/v1/search?limit=1", &[("cookie", &cookie)])
            .await
            .status,
        StatusCode::OK
    );

    // A GET does not sign anyone out: it must not be triggerable by a link.
    let get = send(&f.app, "GET", "/auth/logout", &[("cookie", &cookie)]).await;
    assert_ne!(get.status, StatusCode::NO_CONTENT);
    assert_eq!(
        send(&f.app, "GET", "/v1/search?limit=1", &[("cookie", &cookie)])
            .await
            .status,
        StatusCode::OK
    );

    let out = send(&f.app, "POST", "/auth/logout", &[("cookie", &cookie)]).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(out
        .set_cookies
        .iter()
        .any(|c| c.starts_with("tc_session=;")));
    // The old cookie is dead on the server, not merely cleared in the browser.
    assert_eq!(
        send(&f.app, "GET", "/v1/search?limit=1", &[("cookie", &cookie)])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_made_up_session_cookie_is_worth_nothing() {
    let f = fixture(emails(&["owner@example.com"])).await;
    let r = send(
        &f.app,
        "GET",
        "/v1/search?limit=1",
        &[(
            "cookie",
            "tc_session=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

/// With no provider configured the server is exactly what it was.
#[tokio::test]
async fn with_sign_in_off_nothing_is_guarded() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);

    assert_eq!(
        send(&app, "GET", "/v1/search?limit=1", &[]).await.status,
        StatusCode::OK
    );
    let me = send(&app, "GET", "/auth/me", &[]).await;
    assert_eq!(me.status, StatusCode::OK);
    assert!(me.body.contains("\"off\""));
    // And /auth/login has nowhere to send anyone.
    let login = send(&app, "GET", "/auth/login", &[]).await;
    assert_eq!(login.location.as_deref(), Some("/"));
}

#[tokio::test]
async fn discovery_refuses_a_document_for_a_different_issuer() {
    let provider = start_provider().await;
    let real = provider.base.lock().unwrap().clone();
    // Ask for the same host under a different issuer name.
    let asked = real.replace("127.0.0.1", "localhost");
    let result = OidcClient::discover(OidcConfig {
        issuer: asked,
        client_id: CLIENT_ID.into(),
        client_secret: None,
        public_url: PUBLIC_URL.into(),
        allow: emails(&["owner@example.com"]),
    })
    .await;
    assert!(result.is_err());
}
