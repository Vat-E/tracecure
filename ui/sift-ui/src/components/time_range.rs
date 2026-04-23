//! From/to datetime-local inputs plus three quick-range buttons.
//!
//! The component is controlled: the parent holds the two RwSignals and
//! receives a callback when the user clicks a quick-range button. The parent
//! decides what to do with the populated values (e.g. submit the search).
//!
//! Time semantics: picker values are UTC. Users who want local time can
//! type seconds and a timezone into the input directly; the picker's native
//! behaviour is local but we convert to UTC RFC3339 at submit time.

use leptos::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuickRange {
    OneHour,
    OneDay,
    SevenDays,
}

impl QuickRange {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneHour => "Last 1h",
            Self::OneDay => "Last 24h",
            Self::SevenDays => "Last 7d",
        }
    }
    /// Number of seconds back from `now` the range starts.
    pub fn seconds(self) -> i64 {
        match self {
            Self::OneHour => 3_600,
            Self::OneDay => 86_400,
            Self::SevenDays => 7 * 86_400,
        }
    }
}

#[component]
pub fn TimeRange(
    from: RwSignal<String>,
    to: RwSignal<String>,
    /// Called when a quick-range button is clicked, AFTER from/to are populated.
    on_quick: Callback<QuickRange>,
) -> impl IntoView {
    let click_quick = move |r: QuickRange| {
        let (lo, hi) = compute_range(r);
        from.set(lo);
        to.set(hi);
        on_quick.call(r);
    };

    view! {
        <div class="time-range">
            <label>"From"</label>
            <input
                type="datetime-local"
                step="1"
                prop:value=move || from.get()
                on:input=move |ev| from.set(event_target_value(&ev))
            />
            <label>"To"</label>
            <input
                type="datetime-local"
                step="1"
                prop:value=move || to.get()
                on:input=move |ev| to.set(event_target_value(&ev))
            />
            <div class="quick-ranges">
                <button type="button" on:click=move |_| click_quick(QuickRange::OneHour)>"Last 1h"</button>
                <button type="button" on:click=move |_| click_quick(QuickRange::OneDay)>"Last 24h"</button>
                <button type="button" on:click=move |_| click_quick(QuickRange::SevenDays)>"Last 7d"</button>
            </div>
        </div>
    }
}

/// Compute (from, to) strings for the datetime-local input, UTC.
/// Return format: "YYYY-MM-DDTHH:MM:SS" (no timezone marker — picker doesn't accept Z).
pub(crate) fn compute_range(r: QuickRange) -> (String, String) {
    let now_ms = js_sys::Date::now() as i64; // epoch ms, UTC
    let lo_ms = now_ms - r.seconds() * 1000;
    (format_utc(lo_ms), format_utc(now_ms))
}

fn format_utc(ms: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        d.get_utc_full_year(),
        d.get_utc_month() + 1,
        d.get_utc_date(),
        d.get_utc_hours(),
        d.get_utc_minutes(),
        d.get_utc_seconds(),
    )
}
