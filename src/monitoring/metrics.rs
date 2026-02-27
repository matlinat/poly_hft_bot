use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use serde::Serialize;
use tracing::info;

/// Global metrics registry used across the bot.
pub static METRICS: Lazy<Metrics> = Lazy::new(Metrics::default);

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

#[derive(Default)]
struct MetricsInner {
    snapshots_recorded: AtomicU64,
    orders_submitted: AtomicU64,
    orders_failed: AtomicU64,
    last_event_ts: AtomicU64,
}

/// Lightweight metrics handle backed by atomics so it can be cloned cheaply.
#[derive(Clone, Default)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

impl Metrics {
    pub fn record_snapshot(&self, market_slug: &str) {
        self.inner.snapshots_recorded.fetch_add(1, Ordering::Relaxed);
        self.inner
            .last_event_ts
            .store(now_unix_secs(), Ordering::Relaxed);

        info!(
            target: "metrics",
            event = "snapshot",
            market = %market_slug,
            total_snapshots = self.inner.snapshots_recorded.load(Ordering::Relaxed),
            "snapshot recorded"
        );
    }

    pub fn record_order_submitted(&self, market_slug: &str, leg: &str) {
        self.inner.orders_submitted.fetch_add(1, Ordering::Relaxed);
        self.inner
            .last_event_ts
            .store(now_unix_secs(), Ordering::Relaxed);

        info!(
            target: "metrics",
            event = "order_submitted",
            market = %market_slug,
            leg = %leg,
            total_orders = self.inner.orders_submitted.load(Ordering::Relaxed),
            "order submitted"
        );
    }

    pub fn record_order_failed(&self, market_slug: &str, reason: &str) {
        self.inner.orders_failed.fetch_add(1, Ordering::Relaxed);
        self.inner
            .last_event_ts
            .store(now_unix_secs(), Ordering::Relaxed);

        info!(
            target: "metrics",
            event = "order_failed",
            market = %market_slug,
            reason = %reason,
            total_failures = self.inner.orders_failed.load(Ordering::Relaxed),
            "order failed"
        );
    }

    pub fn heartbeat(&self) {
        self.inner
            .last_event_ts
            .store(now_unix_secs(), Ordering::Relaxed);
    }

    pub fn is_healthy(&self, max_staleness: Duration) -> bool {
        let last = self.inner.last_event_ts.load(Ordering::Relaxed);
        if last == 0 {
            // If we have never seen an event, treat as healthy immediately after startup.
            return true;
        }
        let now = now_unix_secs();
        now.saturating_sub(last) <= max_staleness.as_secs()
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            snapshots_recorded: self
                .inner
                .snapshots_recorded
                .load(Ordering::Relaxed),
            orders_submitted: self.inner.orders_submitted.load(Ordering::Relaxed),
            orders_failed: self.inner.orders_failed.load(Ordering::Relaxed),
            last_event_ts: self.inner.last_event_ts.load(Ordering::Relaxed),
        }
    }
}

/// Serializable view of current metrics used by dashboards and health checks.
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub snapshots_recorded: u64,
    pub orders_submitted: u64,
    pub orders_failed: u64,
    pub last_event_ts: u64,
}

pub fn log_metrics_snapshot(snapshot: &MetricsSnapshot) {
    info!(
        target: "metrics",
        event = "metrics_snapshot",
        snapshots_recorded = snapshot.snapshots_recorded,
        orders_submitted = snapshot.orders_submitted,
        orders_failed = snapshot.orders_failed,
        last_event_ts = snapshot.last_event_ts,
        "metrics snapshot"
    );
}

