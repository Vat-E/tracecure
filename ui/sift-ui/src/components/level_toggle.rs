//! Level visibility pill row. Lives in the top shell so it applies to both
//! tail and search result lists.

use leptos::*;

#[derive(Clone, Copy, Debug)]
pub struct LevelSet {
    pub trace: bool,
    pub debug: bool,
    pub info: bool,
    pub warn: bool,
    pub error: bool,
    pub fatal: bool,
}

impl Default for LevelSet {
    fn default() -> Self {
        // Visible by default: info, warn, error, fatal.
        Self {
            trace: false,
            debug: false,
            info: true,
            warn: true,
            error: true,
            fatal: true,
        }
    }
}

impl LevelSet {
    pub fn allows(&self, lvl: &str) -> bool {
        match lvl {
            "trace" => self.trace,
            "debug" => self.debug,
            "info" => self.info,
            "warn" => self.warn,
            "error" => self.error,
            "fatal" => self.fatal,
            _ => true, // Unknown levels stay visible.
        }
    }
}

#[component]
pub fn LevelToggle(set: RwSignal<LevelSet>) -> impl IntoView {
    let show_more = create_rw_signal(false);

    view! {
        <div class="level-toggle">
            <button type="button"
                class=move || if set.get().info  { "level-pill on" } else { "level-pill off" }
                on:click=move |_| set.update(|s| s.info = !s.info)>"info"</button>
            <button type="button"
                class=move || if set.get().warn  { "level-pill on" } else { "level-pill off" }
                on:click=move |_| set.update(|s| s.warn = !s.warn)>"warn"</button>
            <button type="button"
                class=move || if set.get().error { "level-pill on" } else { "level-pill off" }
                on:click=move |_| set.update(|s| s.error = !s.error)>"error"</button>
            <button type="button"
                class=move || if set.get().fatal { "level-pill on" } else { "level-pill off" }
                on:click=move |_| set.update(|s| s.fatal = !s.fatal)>"fatal"</button>
            <button type="button" class="level-more"
                on:click=move |_| show_more.update(|v| *v = !*v)>
                { move || if show_more.get() { "less" } else { "more…" } }
            </button>
            { move || if show_more.get() {
                view! {
                    <>
                    <button type="button"
                        class=move || if set.get().debug { "level-pill on" } else { "level-pill off" }
                        on:click=move |_| set.update(|s| s.debug = !s.debug)>"debug"</button>
                    <button type="button"
                        class=move || if set.get().trace { "level-pill on" } else { "level-pill off" }
                        on:click=move |_| set.update(|s| s.trace = !s.trace)>"trace"</button>
                    </>
                }.into_view()
            } else {
                view! { <span></span> }.into_view()
            }}
        </div>
    }
}
