//! Status pill with an optional "Retry now" button during reconnect.

use leptos::*;

use crate::ws::{RetryHandle, Status};

#[component]
pub fn StatusPill(status: RwSignal<Status>, retry: RetryHandle) -> impl IntoView {
    view! {
        <div class="status-pill-wrap">
            <span class=move || status.get().css()>
                { move || status.get().label() }
            </span>
            { move || if status.get().is_reconnecting() {
                let retry = retry.clone();
                let retry_click = move |_| retry.retry_now();
                view! {
                    <button type="button" class="retry-btn" on:click=retry_click>"Retry now"</button>
                }.into_view()
            } else {
                view! { <></> }.into_view()
            }}
        </div>
    }
}
