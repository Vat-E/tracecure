//! Help overlay. Dialog with a keybinding table; visibility is controlled
//! by the caller's RwSignal<bool>.

use leptos::*;

#[component]
pub fn HelpModal(open: RwSignal<bool>) -> impl IntoView {
    let close = move |_| open.set(false);

    view! {
        { move || if open.get() {
            view! {
                <div class="help-backdrop" on:click=close>
                    <div class="help-dialog" on:click=|ev| ev.stop_propagation()>
                        <header>
                            <h2>"Keybindings"</h2>
                            <button class="close" on:click=close>"×"</button>
                        </header>
                        <section>
                            <h3>"Navigation"</h3>
                            <table>
                                <tr><td class="key">"Tail tab"</td><td>"click header"</td></tr>
                                <tr><td class="key">"Search tab"</td><td>"click header"</td></tr>
                                <tr><td class="key">"?"</td><td>"toggle this help"</td></tr>
                                <tr><td class="key">"Esc"</td><td>"close modal / detail panel"</td></tr>
                            </table>
                        </section>
                        <section>
                            <h3>"Tail"</h3>
                            <table>
                                <tr><td class="key">"click row"</td><td>"open detail panel"</td></tr>
                                <tr><td class="key">"type in filter"</td><td>"substring filter (body)"</td></tr>
                                <tr><td class="key">"Pause button"</td><td>"pause/resume live updates"</td></tr>
                            </table>
                        </section>
                        <section>
                            <h3>"Search"</h3>
                            <table>
                                <tr><td class="key">"Enter in query"</td><td>"run search"</td></tr>
                                <tr><td class="key">"Last 1h/24h/7d"</td><td>"populate time range + submit"</td></tr>
                                <tr><td class="key">"click row"</td><td>"open detail panel"</td></tr>
                            </table>
                        </section>
                        <section>
                            <h3>"Detail panel"</h3>
                            <table>
                                <tr><td class="key">"Esc"</td><td>"close"</td></tr>
                                <tr><td class="key">"× button"</td><td>"close"</td></tr>
                                <tr><td class="key">"click outside"</td><td>"close"</td></tr>
                            </table>
                        </section>
                        <section>
                            <h3>"Level toggle (top bar)"</h3>
                            <table>
                                <tr><td class="key">"click pill"</td><td>"hide/show that level in tail + search"</td></tr>
                                <tr><td class="key">"more…"</td><td>"expose trace/debug pills"</td></tr>
                            </table>
                        </section>
                    </div>
                </div>
            }.into_view()
        } else {
            view! { <></> }.into_view()
        }}
    }
}
