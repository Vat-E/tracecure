//! `/auth/*` — signing in through the OIDC provider — and the guard that
//! keeps everything that reads logs behind a session.

use axum::extract::{Query, Request, State};
use axum::http::header::{ACCEPT, COOKIE, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::oidc::{code_challenge, random_token};
use crate::session::{
    clear_cookie, cookie, safe_next, set_cookie, PendingLogin, Session, LOGIN_COOKIE, LOGIN_TTL,
    SESSION_COOKIE, SESSION_TTL,
};
use crate::state::{AppState, Auth};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|h| cookie(h, name))
}

fn session_of(auth: &Auth, headers: &HeaderMap) -> Option<Session> {
    auth.sessions.get(cookie_value(headers, SESSION_COOKIE)?)
}

fn redirect(to: &str) -> Response {
    (StatusCode::SEE_OTHER, [(LOCATION, to.to_string())]).into_response()
}

/// A small page for the cases where sign-in ends without a session. Plain
/// HTML with the message escaped: it can contain an e-mail address from the
/// provider.
fn message_page(status: StatusCode, title: &str, detail: &str) -> Response {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{t} · Tracecure</title>\
         <body style=\"font:15px/1.5 system-ui,sans-serif;background:#1a1918;color:#f5f4ee;\
         display:grid;place-items:center;height:100vh;margin:0\">\
         <main style=\"max-width:32rem;padding:2rem\"><h1 style=\"font-size:1.2rem\">{t}</h1>\
         <p style=\"color:#c2c0b6\">{d}</p>\
         <p><a style=\"color:#e59a80\" href=\"/auth/login\">Try signing in again</a></p></main>",
        t = esc(title),
        d = esc(detail),
    );
    (status, [("content-type", "text/html; charset=utf-8")], body).into_response()
}

#[derive(Deserialize)]
struct LoginParams {
    next: Option<String>,
}

/// Start a sign-in: remember where to come back to, and send the browser to
/// the provider.
async fn login(State(state): State<AppState>, Query(params): Query<LoginParams>) -> Response {
    let Some(auth) = state.auth() else {
        // No provider configured: there is nothing to sign in to.
        return redirect("/");
    };
    let (Ok(login_state), Ok(verifier)) = (random_token(32), random_token(32)) else {
        return message_page(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Sign-in is unavailable",
            "The server could not generate a secure random value.",
        );
    };
    let url = match auth
        .oidc
        .authorize_url(&login_state, &code_challenge(&verifier))
    {
        Ok(url) => url,
        Err(e) => {
            tracing::error!(%e, "cannot build the authorization URL");
            return message_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Sign-in is unavailable",
                "The identity provider is misconfigured. The server log has the detail.",
            );
        }
    };
    auth.sessions.begin_login(
        login_state.clone(),
        PendingLogin {
            code_verifier: verifier,
            next: safe_next(params.next.as_deref()),
        },
    );
    // The state also goes into a cookie. The callback requires the two to
    // match, which ties the response to the browser that started the flow:
    // a link carrying someone else's `code` and `state` fails here.
    let secure = auth.oidc.config.secure_cookies();
    (
        StatusCode::SEE_OTHER,
        [
            (LOCATION, url),
            (
                SET_COOKIE,
                set_cookie(LOGIN_COOKIE, &login_state, LOGIN_TTL, secure),
            ),
        ],
    )
        .into_response()
}

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// The provider sends the browser back here.
async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Response {
    let Some(auth) = state.auth() else {
        return redirect("/");
    };
    let secure = auth.oidc.config.secure_cookies();
    let forget_login = (SET_COOKIE, clear_cookie(LOGIN_COOKIE, secure));

    if let Some(error) = params.error {
        tracing::warn!(%error, detail = ?params.error_description, "provider refused sign-in");
        let mut resp = message_page(
            StatusCode::FORBIDDEN,
            "Sign-in was not completed",
            params
                .error_description
                .as_deref()
                .unwrap_or("The identity provider did not sign you in."),
        );
        resp.headers_mut()
            .insert(forget_login.0, forget_login.1.parse().expect("ascii"));
        return resp;
    }

    let bad_request = |why: &'static str| {
        tracing::warn!(why, "sign-in callback rejected");
        let mut resp = message_page(
            StatusCode::BAD_REQUEST,
            "Sign-in could not be verified",
            "This sign-in link is not valid, or it has expired or already been used.",
        );
        resp.headers_mut().insert(
            SET_COOKIE,
            clear_cookie(LOGIN_COOKIE, secure).parse().expect("ascii"),
        );
        resp
    };

    let (Some(code), Some(returned_state)) = (params.code, params.state) else {
        return bad_request("missing code or state");
    };
    if cookie_value(&headers, LOGIN_COOKIE) != Some(returned_state.as_str()) {
        return bad_request("state does not match this browser's sign-in");
    }
    let Some(pending) = auth.sessions.finish_login(&returned_state) else {
        return bad_request("unknown, expired or reused state");
    };

    let user = match auth.oidc.identify(&code, &pending.code_verifier).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!(%e, "code exchange or userinfo failed");
            return bad_request("provider did not confirm the sign-in");
        }
    };

    if let Err(reason) = auth.oidc.config.allow.check(&user) {
        tracing::warn!(sub = %user.sub, email = ?user.email, %reason, "sign-in not allowed");
        let mut resp = message_page(StatusCode::FORBIDDEN, "Not allowed", &reason);
        resp.headers_mut()
            .insert(forget_login.0, forget_login.1.parse().expect("ascii"));
        return resp;
    }

    let Ok(session_id) = random_token(32) else {
        return message_page(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Sign-in is unavailable",
            "The server could not generate a secure random value.",
        );
    };
    tracing::info!(sub = %user.sub, email = ?user.email, "signed in");
    auth.sessions.create(session_id.clone(), user);

    let mut resp = redirect(&pending.next);
    let headers = resp.headers_mut();
    headers.append(
        SET_COOKIE,
        set_cookie(SESSION_COOKIE, &session_id, SESSION_TTL, secure)
            .parse()
            .expect("ascii"),
    );
    headers.append(
        SET_COOKIE,
        clear_cookie(LOGIN_COOKIE, secure).parse().expect("ascii"),
    );
    resp
}

/// End the session. A `POST`, so that a link or an image tag on another
/// site cannot sign someone out.
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(auth) = state.auth() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    if let Some(id) = cookie_value(&headers, SESSION_COOKIE) {
        auth.sessions.remove(id);
    }
    (
        StatusCode::NO_CONTENT,
        [(
            SET_COOKIE,
            clear_cookie(SESSION_COOKIE, auth.oidc.config.secure_cookies()),
        )],
    )
        .into_response()
}

/// Who is signed in. The UI asks this to show the name and to tell "not
/// signed in" apart from "the server is down".
async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(auth) = state.auth() else {
        return Json(serde_json::json!({ "auth": "off" })).into_response();
    };
    match session_of(auth, &headers) {
        Some(session) => Json(serde_json::json!({
            "auth": "oidc",
            "name": session.user.display_name(),
            "email": session.user.email,
        }))
        .into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "auth": "oidc", "login": "/auth/login" })),
        )
            .into_response(),
    }
}

/// Middleware for every route that reads logs or serves the UI.
///
/// With no provider configured it does nothing — the server behaves as it
/// always has. With one, a request without a live session is turned away:
/// a page load is redirected to sign-in and brought back afterwards; an API
/// call gets a `401`, because a redirect to an HTML page is useless to
/// `fetch` and to a WebSocket upgrade.
pub async fn require_session(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let Some(auth) = state.auth() else {
        return next.run(req).await;
    };
    if session_of(auth, req.headers()).is_some() {
        return next.run(req).await;
    }

    let path = req.uri().path();
    let wants_html = req
        .headers()
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));
    if !path.starts_with("/v1/") && wants_html && req.method() == axum::http::Method::GET {
        let target = req
            .uri()
            .path_and_query()
            .map_or("/", axum::http::uri::PathAndQuery::as_str);
        let mut to = reqwest::Url::parse("http://x/auth/login").expect("static");
        to.query_pairs_mut()
            .append_pair("next", &safe_next(Some(target)));
        let location = format!("{}?{}", to.path(), to.query().unwrap_or_default());
        return redirect(&location);
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": "authentication required",
            "login": "/auth/login",
        })),
    )
        .into_response()
}
