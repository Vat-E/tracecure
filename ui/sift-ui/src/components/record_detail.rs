//! Right-side slide-in panel showing a single record in full.

use leptos::*;

use crate::components::json_highlight::JsonHighlight;
use crate::components::record_row::RowRecord;

#[component]
pub fn RecordDetail(
    /// Currently-open record, or `None` to hide the panel.
    open: RwSignal<Option<RowRecord>>,
) -> impl IntoView {
    let close = move |_| open.set(None);

    view! {
        { move || {
            let Some(r) = open.get() else {
                return view! { <div></div> }.into_view();
            };
            let fields_json: serde_json::Value = serde_json::to_value(&r.fields).unwrap_or_default();
            view! {
                <div class="detail-backdrop" on:click=close></div>
                <aside class="detail-panel" on:click=|ev| ev.stop_propagation()>
                    <header class="detail-header">
                        <span>{r.timestamp.clone()}</span>
                        <span>"·"</span>
                        <span>{r.host.clone()}</span>
                        <span>"·"</span>
                        <span>{r.app.clone()}</span>
                        <span>"·"</span>
                        <span>{r.stream_id.clone()}</span>
                        <span>"·"</span>
                        <span class=format!("lvl {}", r.level)>{r.level.clone()}</span>
                        <button class="close" on:click=close>"×"</button>
                    </header>
                    <section>
                        <h3>"Body"</h3>
                        <pre class="body">{r.body.clone()}</pre>
                    </section>
                    <section>
                        <h3>"Labels"</h3>
                        { if r.labels.is_empty() {
                            view! { <div class="empty">"(none)"</div> }.into_view()
                        } else {
                            view! {
                                <table class="labels">
                                    <tbody>
                                        {r.labels.iter().map(|(k, v)| view! {
                                            <tr><td>{k.clone()}</td><td>{v.clone()}</td></tr>
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            }.into_view()
                        }}
                    </section>
                    <section>
                        <h3>"Fields"</h3>
                        { if r.fields.is_empty() {
                            view! { <div class="empty">"(none)"</div> }.into_view()
                        } else {
                            view! { <JsonHighlight value=fields_json /> }.into_view()
                        }}
                    </section>
                    { r.raw.clone().map(|raw| view! {
                        <section>
                            <h3>"Raw"</h3>
                            <pre class="raw">{raw}</pre>
                        </section>
                    })}
                </aside>
            }.into_view()
        }}
    }
}
