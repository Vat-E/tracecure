//! Top-level app shell: sticky header with tabs + <Outlet/> for the active route.

use leptos::*;
use leptos_router::{Route, Router, Routes, A};

use crate::routes::search::Search;
use crate::routes::tail::Tail;

#[component]
pub fn App() -> impl IntoView {
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
        </Router>
    }
}
