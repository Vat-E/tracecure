//! Pause/resume toggle for the live tail. Parent owns both signals.

use leptos::*;

#[component]
pub fn PauseButton(
    paused: RwSignal<bool>,
    pending_count: RwSignal<usize>,
    /// Called when the user resumes. Parent should flush pending into the main list.
    on_resume: Callback<()>,
) -> impl IntoView {
    let label = move || {
        if paused.get() {
            let n = pending_count.get();
            if n == 0 {
                "Resume".to_string()
            } else {
                format!("Resume ({n} new)")
            }
        } else {
            "Pause".to_string()
        }
    };

    let click = move |_| {
        if paused.get() {
            paused.set(false);
            on_resume.call(());
        } else {
            paused.set(true);
        }
    };

    view! {
        <button type="button" class="pause-button" on:click=click>
            { label }
        </button>
    }
}
