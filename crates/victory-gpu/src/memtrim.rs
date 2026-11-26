//! Budget only disposable host caches. Never decommit live guest data or
//! reclaim resources held by a command list or an unfinished fence.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static RELEASED: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

fn limited(legacy: u64, budget: Option<usize>, divisor: u64) -> u64 {
    budget.map_or(legacy, |n| legacy.min(n as u64 / divisor))
}
fn budget() -> Option<usize> {
    static BUDGET: OnceLock<Option<usize>> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let enabled = std::env::var_os("STARPORT_TRIM_CACHES").is_none_or(|v| v != "0");
        let budget = enabled.then(crate::console::memory_override).flatten();
        if let Some(n) = budget {
            tracing::info!(target:"starport::cache_budget", budget_mb=n>>20,
                snapshot_pool_cap_mb=limited(512<<20,Some(n),64)>>20,
                upload_pool_cap_mb=limited(128<<20,Some(n),128)>>20,
                "strict caps on idle host caches; live guest allocations and in-flight staging are preserved; TRIM_CACHES=0 restores legacy caps");
        }
        budget
    })
}
pub(crate) fn active() -> bool {
    budget().is_some()
}
pub(crate) fn snapshots(legacy: u64) -> u64 {
    limited(legacy, budget(), 64)
}
pub(crate) fn uploads(legacy: u64) -> u64 {
    limited(legacy, budget(), 128)
}
pub(crate) fn released(which: usize, bytes: u64) {
    RELEASED[which].fetch_add(bytes, Relaxed);
}
pub(crate) fn report() {
    if !active() {
        return;
    }
    static LAST: AtomicU64 = AtomicU64::new(0);
    let snapshot = RELEASED[0].load(Relaxed);
    let upload = RELEASED[1].load(Relaxed);
    let total = snapshot.saturating_add(upload);
    if LAST.swap(total, Relaxed) != total {
        tracing::info!(target:"starport::cache_budget", released_snapshot_bytes=snapshot,
            released_upload_bytes=upload,"cumulative disposable cache bytes released, not a measured working-set reduction");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lower_reported_budget_also_lowers_disposable_cache_caps() {
        assert_eq!(limited(512 << 20, Some(8192 << 20), 64), 128 << 20);
        assert_eq!(limited(512 << 20, Some(6144 << 20), 64), 96 << 20);
        assert_eq!(limited(128 << 20, Some(6144 << 20), 128), 48 << 20);
        assert_eq!(limited(128 << 20, None, 128), 128 << 20);
        assert_eq!(limited(128 << 20, Some(65536 << 20), 128), 128 << 20);
    }
}
