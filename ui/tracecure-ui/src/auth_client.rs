//! The browser's side of sign-in.
//!
//! The server decides whether sign-in is required and does all of it; the UI
//! only has to notice "you are not signed in" and get out of the way — send
//! the browser to `/auth/login` and let it come back to the same view.
//!
//! That can happen at any time, not just at load: a session expires, or the
//! server restarts and forgets it. So every place that talks to the server
//! funnels a `401` here.

use gloo_net::http::Request;
use serde::Deserialize;
use tracecure_ui::urlstate::login_url;

/// What `/auth/me` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whoami {
    /// The server does not require sign-in.
    Off,
    SignedIn {
        name: String,
    },
    SignedOut,
    /// The server could not be asked (it is down, or the network is). Not a
    /// reason to navigate anywhere.
    Unknown,
}

#[derive(Deserialize)]
struct Me {
    auth: String,
    #[serde(default)]
    name: Option<String>,
}

pub async fn whoami() -> Whoami {
    let Ok(resp) = Request::get("/auth/me").send().await else {
        return Whoami::Unknown;
    };
    if resp.status() == 401 {
        return Whoami::SignedOut;
    }
    match resp.json::<Me>().await {
        Ok(me) if me.auth == "off" => Whoami::Off,
        Ok(me) => Whoami::SignedIn {
            name: me.name.unwrap_or_default(),
        },
        // An older server has no `/auth/me`: the UI's `index.html` comes
        // back instead, which is not JSON. That server has no sign-in.
        Err(_) => Whoami::Off,
    }
}

/// Leave for the sign-in page, asking to be brought back to this view.
pub fn go_to_login() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let location = window.location();
    let here = format!(
        "{}{}",
        location.pathname().unwrap_or_else(|_| "/".to_string()),
        location.search().unwrap_or_default()
    );
    let _ = location.assign(&login_url(&here));
}

/// After something failed in a way a missing session would explain — the
/// live tail would not connect — ask, and leave only if that is the reason.
pub fn leave_if_signed_out() {
    wasm_bindgen_futures::spawn_local(async {
        if whoami().await == Whoami::SignedOut {
            go_to_login();
        }
    });
}

/// End the session and reload, which lands on the sign-in page.
pub fn sign_out() {
    wasm_bindgen_futures::spawn_local(async {
        let _ = Request::post("/auth/logout").send().await;
        if let Some(window) = web_sys::window() {
            let _ = window.location().assign("/");
        }
    });
}
