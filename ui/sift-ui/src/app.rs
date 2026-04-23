//! Top-level app shell: sticky header with tabs + <Outlet/> for the active route.

use leptos::*;
use leptos_router::{Route, Router, Routes, A};

use crate::components::record_detail::RecordDetail;
use crate::components::record_row::RowRecord;
use crate::routes::search::Search;
use crate::routes::tail::Tail;

/// Global "currently open detail record" — `None` means the panel is hidden.
#[derive(Clone, Copy)]
pub struct OpenDetail(pub RwSignal<Option<RowRecord>>);

#[component]
pub fn App() -> impl IntoView {
    let open_detail = create_rw_signal::<Option<RowRecord>>(None);
    provide_context(OpenDetail(open_detail));

    // Global Esc handler to close the detail panel.
    window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" {
            open_detail.set(None);
        }
    });

    view! {
        <Router>
            <header>
                <h1>"sift"</h1>
                <nav class="tabs">
                    <A href="/tail" exact=true>"Tail"</A>
                    <A href="/search">"Search"</A>
                </nav>
            </header>
            <main>
                <Routes>
                    <Route path="/" view=|| view! { <Tail/> } />
                    <Route path="/tail" view=|| view! { <Tail/> } />
                    <Route path="/search" view=|| view! { <Search/> } />
                </Routes>
            </main>
            <RecordDetail open=open_detail />
        </Router>
    }
}
