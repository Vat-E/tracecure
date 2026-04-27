//! App shell — minimal skeleton during migration. Task 7 fills this in
//! with the new router + `AppState` signal + component tree.

use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <div style="padding:20px;color:#ccc;background:#111;height:100vh;font-family:monospace">
            "Sift UI — under reconstruction (Leptos 0.8 migration)"
        </div>
    }
}
