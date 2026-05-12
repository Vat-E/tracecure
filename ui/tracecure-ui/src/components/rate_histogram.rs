//! Rate histogram — 30-bucket sparkline between the top bar and the
//! stream. Warn-tinted bars for buckets containing warn records;
//! error-tinted for error/fatal; default lavender wash otherwise.
//!
//! Pure client-side: buckets are computed from `state.records` against
//! the current `state.time_window`. Recompute on each reactive read.

use leptos::prelude::*;
use tracecure_ui::state::{RateBucket, TimeWindow};

use crate::app::AppCtx;

#[component]
pub fn RateHistogram() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    // Window bounds + label as a single reactive read.
    let window_info = move || -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>, String) {
        state.with(|s| {
            let now = chrono::Utc::now();
            match &s.time_window {
                TimeWindow::Preset(d) => {
                    let from = now
                        - chrono::Duration::from_std(*d)
                            .unwrap_or_else(|_| chrono::Duration::seconds(0));
                    (from, now, format_window(*d))
                }
                TimeWindow::Custom { from, to } => (*from, *to, "custom".to_string()),
            }
        })
    };

    let buckets = move || -> (Vec<RateBucket>, u32) {
        let (from, to, _) = window_info();
        state.with(|s| {
            let bs = s.rate_buckets(from, to);
            let max = bs.iter().map(|b| b.total).max().unwrap_or(1).max(1);
            (bs, max)
        })
    };

    let avg_rate = move || -> u32 {
        let (from, to, _) = window_info();
        state.with(|s| {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            {
                let total = s.records.len() as f64;
                let mins = ((to - from).num_seconds() as f64 / 60.0).max(1.0);
                (total / mins).round() as u32
            }
        })
    };

    view! {
        <div class="histogram-strip">
            <span class="hlabel">{move || format!("rate · {}", window_info().2)}</span>
            <div class="sparkline">
                {move || {
                    let (bs, max) = buckets();
                    bs.into_iter()
                        .map(|b| {
                            #[allow(clippy::cast_precision_loss)]
                            let pct = ((f64::from(b.total) / f64::from(max)) * 100.0).clamp(0.0, 100.0);
                            let kind = if b.error_count > 0 { "err" }
                                       else if b.warn_count > 0 { "warn" }
                                       else { "" };
                            view! {
                                <div class=format!("b {kind}")
                                     style=format!("height: {pct}%")></div>
                            }
                        })
                        .collect_view()
                }}
            </div>
            <span class="hcount"><strong>{move || format!("~{}", avg_rate())}</strong>"/min"</span>
        </div>
    }
}

fn format_window(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}
