use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::api::state::AppState;
use crate::ingest::publisher;

#[derive(Default)]
pub struct AppMetrics {
    ingest_requests_total: AtomicU64,
    ingest_request_duration_us_sum: AtomicU64,
    ingest_request_duration_us_count: AtomicU64,
    ingest_events_accepted_total: AtomicU64,
    ingest_events_duplicates_total: AtomicU64,
    ingest_auth_rejections_total: AtomicU64,
    ws_connections_total: AtomicU64,
    ws_messages_sent_total: AtomicU64,
    ws_errors_total: AtomicU64,
}

impl AppMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inc_ingest_requests(&self) {
        self.ingest_requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn observe_ingest_request_duration_us(&self, duration_us: u64) {
        self.ingest_request_duration_us_sum
            .fetch_add(duration_us, Ordering::Relaxed);
        self.ingest_request_duration_us_count
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_ingest_events_accepted(&self, count: u64) {
        self.ingest_events_accepted_total
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_ingest_events_duplicates(&self, count: u64) {
        self.ingest_events_duplicates_total
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn inc_ingest_auth_rejections(&self) {
        self.ingest_auth_rejections_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_ws_connections(&self) {
        self.ws_connections_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_ws_messages_sent(&self) {
        self.ws_messages_sent_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_ws_errors(&self) {
        self.ws_errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn render_prometheus(&self) -> String {
        let mut out = String::new();
        append_counter(
            &mut out,
            "ingest_requests_total",
            self.ingest_requests_total.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ingest_request_duration_us_sum",
            self.ingest_request_duration_us_sum.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ingest_request_duration_us_count",
            self.ingest_request_duration_us_count
                .load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ingest_events_accepted_total",
            self.ingest_events_accepted_total.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ingest_events_duplicates_total",
            self.ingest_events_duplicates_total.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ingest_auth_rejections_total",
            self.ingest_auth_rejections_total.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ws_connections_total",
            self.ws_connections_total.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ws_messages_sent_total",
            self.ws_messages_sent_total.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "ws_errors_total",
            self.ws_errors_total.load(Ordering::Relaxed),
        );
        append_counter(
            &mut out,
            "dlq_published_total",
            publisher::dlq_published_total(),
        );
        out
    }
}

fn append_counter(out: &mut String, name: &str, value: u64) {
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push_str(" counter\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

pub async fn metrics_handler(State(state): State<AppState>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        state.metrics.render_prometheus(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_render_includes_key_metrics() {
        let metrics = AppMetrics::new();
        let rendered = metrics.render_prometheus();

        assert!(rendered.contains("ingest_requests_total"));
        assert!(rendered.contains("ingest_request_duration_us_sum"));
        assert!(rendered.contains("ingest_request_duration_us_count"));
        assert!(rendered.contains("ingest_events_accepted_total"));
        assert!(rendered.contains("ingest_events_duplicates_total"));
        assert!(rendered.contains("ingest_auth_rejections_total"));
        assert!(rendered.contains("ws_connections_total"));
        assert!(rendered.contains("ws_messages_sent_total"));
        assert!(rendered.contains("ws_errors_total"));
        assert!(rendered.contains("dlq_published_total"));
    }
}
