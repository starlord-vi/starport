//! No-present diagnostics. Fixed-size events, no guest memory probes and no
//! renderer locks held while emitting logs or requesting an optional stack dump.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Default, Debug)]
struct Event {
    ms: u64,
    thread: u32,
    kind: u8,
    a: u64,
    b: u64,
}
struct Ring {
    events: [Event; 200],
    next: usize,
    count: usize,
}
static RING: Mutex<Ring> = Mutex::new(Ring {
    events: [Event {
        ms: 0,
        thread: 0,
        kind: 0,
        a: 0,
        b: 0,
    }; 200],
    next: 0,
    count: 0,
});
static LAST_PRESENT: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static START: OnceLock<Instant> = OnceLock::new();
static CALLBACK: OnceLock<fn()> = OnceLock::new();

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("STARPORT_WATCHDOG").is_none_or(|v| v != "0"))
}
fn ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
fn timed_out(now: u64, last: u64, previous_dump: u64, timeout: u64) -> bool {
    now.saturating_sub(last) >= timeout && now.saturating_sub(previous_dump) >= timeout
}
pub(crate) fn event(kind: u8, a: u64, b: u64) {
    if !enabled() {
        return;
    }
    // Diagnostics never block a recording/submission thread on the history lock.
    let Ok(mut ring) = RING.try_lock() else {
        DROPPED.fetch_add(1, Relaxed);
        return;
    };
    let at = ring.next;
    ring.events[at] = Event {
        ms: ms(),
        thread: unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        kind,
        a,
        b,
    };
    ring.next = (at + 1) % 200;
    ring.count = (ring.count + 1).min(200);
}
pub(crate) fn presented() {
    LAST_PRESENT.store(ms(), Relaxed);
}

/// The loader supplies its existing preallocated suspend/unwind/resume helper.
/// It is called only when STARPORT_WATCHDOG_STACKS=1 (default off).
pub fn on_stacks(callback: fn()) {
    let _ = CALLBACK.set(callback);
}

pub fn start() {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if !enabled() || RUNNING.swap(true, Relaxed) {
        return;
    }
    LAST_PRESENT.store(ms(), Relaxed);
    let seconds = std::env::var("STARPORT_WATCHDOG_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| (1..=600).contains(n))
        .unwrap_or(10);
    let stacks = std::env::var_os("STARPORT_WATCHDOG_STACKS").is_some_and(|v| v == "1");
    tracing::info!(target:"starport::watchdog",seconds,stacks,"no-present watchdog armed; 0 disables STARPORT_WATCHDOG");
    std::thread::Builder::new().name("starport-watchdog".into()).spawn(move || {
        let mut last_dump=0;
        loop {
            std::thread::sleep(Duration::from_secs(1));let now=ms();let last=LAST_PRESENT.load(Relaxed);
            if !timed_out(now,last,last_dump,seconds*1000) {continue;}
            last_dump=now;
            crate::host::watchdog_snapshot(now.saturating_sub(last));
            tracing::warn!(target:"starport::watchdog",dropped_history_events=DROPPED.load(Relaxed),"bounded nonblocking history");
            let history=RING.try_lock().ok().map(|ring| {
                (0..ring.count).map(|i|ring.events[(ring.next+200-ring.count+i)%200]).collect::<Vec<_>>()
            });
            if let Some(history)=history {
                for event in history {
                    tracing::warn!(target:"starport::watchdog",ago_ms=now.saturating_sub(event.ms),thread=event.thread,
                        kind=event.kind,a=event.a,b=event.b,"recent event (1 phase, 2 dispatch, 3 SRV, 4 UAV, 5 fence signal, 6 queue wait)");
                }
            } else {tracing::warn!(target:"starport::watchdog","event ring busy; history unavailable");}
            if stacks {
                if let Some(callback)=CALLBACK.get() {callback();}
                else {tracing::warn!(target:"starport::watchdog","loader stack callback unavailable");}
            }
        }
    }).unwrap_or_else(|error|crate::host::fail(format_args!("cannot start watchdog: {error}")));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watchdog_rate_limits_dumps_and_resets_after_progress() {
        assert!(!timed_out(9999, 0, 0, 10000));
        assert!(timed_out(10000, 0, 0, 10000));
        assert!(!timed_out(15000, 0, 10000, 10000));
        assert!(timed_out(20000, 0, 10000, 10000));
        assert!(!timed_out(20000, 19000, 10000, 10000));
        assert!(!timed_out(0, 10, 20, 10000));
    }
}
