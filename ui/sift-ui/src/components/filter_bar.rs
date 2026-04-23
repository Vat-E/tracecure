//! Substring filter over `record.body`. Controlled — parent owns the signal.

use leptos::*;

#[component]
pub fn FilterBar(filter: RwSignal<String>) -> impl IntoView {
    let clear = move |_| filter.set(String::new());

    view! {
        <div class="filter-bar">
            <input
                type="text"
                class="filter-input"
                placeholder="filter body…"
                prop:value=move || filter.get()
                on:input=move |ev| filter.set(event_target_value(&ev))
            />
            { move || if !filter.get().is_empty() {
                view! {
                    <button type="button" class="filter-clear" on:click=clear>"×"</button>
                }.into_view()
            } else {
                view! { <span></span> }.into_view()
            }}
        </div>
    }
}
