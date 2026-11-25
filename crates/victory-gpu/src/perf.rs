//! Bounded per-present CPU work samples. Subphase totals sum worker time;
//! they can overlap and must not be subtracted as disjoint wall time.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Instant;

const NAMES: [&str; 10] = [
    "frame",
    "preparing",
    "fence_wait",
    "copy_back",
    "hashing",
    "write_watch",
    "translation",
    "upload_recording",
    "staging_alloc",
    "merge_scan",
];
const CAP: usize = 8192;
static CLOCK: LazyLock<Instant> = LazyLock::new(Instant::now);
static TOTALS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];
static TRIGGER: AtomicU64 = AtomicU64::new(0);
static HELD: AtomicBool = AtomicBool::new(false);

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_SUBPHASES").is_none_or(|v| v != "0"))
}
fn bench_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_BENCH").is_some_and(|v| v == "1"))
}
pub(crate) fn collecting() -> bool {
    enabled() || bench_enabled()
}

#[derive(Clone, Copy)]
pub(crate) enum Part {
    Hashing = 3,
    WriteWatch,
    Translation,
    UploadRecording,
    StagingAlloc,
    MergeScan,
}
pub(crate) struct Timer(usize, Option<Instant>);
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.1 {
            TOTALS[self.0].fetch_add(start.elapsed().as_micros() as u64, Relaxed);
        }
    }
}
pub(crate) fn time(part: Part) -> Timer {
    Timer(
        part as usize,
        (enabled() || bench_enabled()).then(Instant::now),
    )
}
pub(crate) fn phase(which: usize, us: u64) {
    if enabled() || bench_enabled() {
        TOTALS[which].fetch_add(us, Relaxed);
    }
}
/// Called only for F8 messages on a title/owned host window; consumes the key
/// only while bench mode is on. Auto-repeat does not restart a capture.
pub fn hotkey(down: bool) -> bool {
    if !bench_enabled() {
        return false;
    }
    if down && !HELD.swap(true, Relaxed) {
        TRIGGER.store(CLOCK.elapsed().as_micros().max(1) as u64, Relaxed);
    } else if !down {
        HELD.store(false, Relaxed);
    }
    true
}

#[derive(Clone, Copy, Default)]
struct Memory {
    allocations: [u64; 7],
    dxgi: Option<[u64; 4]>,
    physical: Option<[u64; 2]>, // total, available
    process: Option<[u64; 2]>,  // working set, private commit
}
impl Memory {
    fn capture() -> Self {
        use windows_sys::Win32::System::{
            ProcessStatus::*, SystemInformation::*, Threading::GetCurrentProcess,
        };
        let mut physical = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        let mut process = PROCESS_MEMORY_COUNTERS_EX::default();
        // SAFETY: correctly sized writable structures and current-process handle.
        let physical = (unsafe { GlobalMemoryStatusEx(&mut physical) } != 0)
            .then_some([physical.ullTotalPhys, physical.ullAvailPhys]);
        let process = (unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&raw mut process).cast(),
                size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            )
        } != 0)
            .then_some([process.WorkingSetSize as u64, process.PrivateUsage as u64]);
        Self {
            allocations: crate::allocstat::snapshot(),
            dxgi: crate::host::host().and_then(|h| h.video_memory()),
            physical,
            process,
        }
    }
    fn json(self) -> String {
        fn array<const N: usize>(value: Option<[u64; N]>) -> String {
            value.map_or_else(|| "null".into(), |v| format!("{:?}", v))
        }
        format!(
            "{{\"allocation_bytes\":{:?},\"allocation_order\":[\"texture\",\"rt_depth\",\"buffer\",\"upload\",\"readback\",\"descriptor_payload_estimate\",\"pso_objects\"],\"driver_pso_bytes\":null,\"dxgi_budget_usage_local_nonlocal\":{},\"physical_total_available\":{},\"process_working_set_private_commit\":{}}}",
            self.allocations,
            array(self.dxgi),
            array(self.physical),
            array(self.process)
        )
    }
}

#[derive(Default)]
struct Samples {
    rows: VecDeque<[u64; 10]>,
    dropped: u64,
}
impl Samples {
    fn push(&mut self, row: [u64; 10]) {
        if self.rows.len() == CAP {
            self.rows.pop_front();
            self.dropped += 1;
        }
        self.rows.push_back(row);
    }
    fn stats(&self, column: usize) -> (u64, u64) {
        let mut values: Vec<_> = self.rows.iter().map(|r| r[column]).collect();
        values.sort_unstable();
        if values.is_empty() {
            return (0, 0);
        }
        let n = values.len();
        let median = if n.is_multiple_of(2) {
            values[n / 2 - 1] + (values[n / 2] - values[n / 2 - 1]) / 2
        } else {
            values[n / 2]
        };
        (median, values[(n * 95).div_ceil(100) - 1])
    }
    fn json(&self) -> String {
        let values: Vec<_> = NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let (median, p95) = self.stats(i);
                format!("\"{name}\":{{\"median_us\":{median},\"p95_us\":{p95}}}")
            })
            .collect();
        format!("{{{}}}", values.join(","))
    }
}
struct Bench {
    start: u64,
    samples: Samples,
    memory: Memory,
    draws: u64,
}
struct State {
    previous: u64,
    last_log: u64,
    last_histogram: u64,
    frame: u64,
    interval: Samples,
    bench: Option<Bench>,
    memory: Memory,
    previous_draws: u64,
    memory_frame: u64,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        previous: 0,
        last_log: 0,
        last_histogram: 0,
        frame: 0,
        interval: Samples::default(),
        bench: None,
        memory: Memory::default(),
        previous_draws: 0,
        memory_frame: 0,
    })
});

fn write_bench(bench: Bench, memory: Memory, now: u64) {
    let path = std::env::var_os("STARPORT_BENCH_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("starport-bench-{}.json", std::process::id()))
        });
    let result = (|| -> anyhow::Result<()> {
        victory_core::policy::ensure_outside_repo(&path)?;
        let tmp = path.with_extension("json.tmp");
        victory_core::policy::ensure_outside_repo(&tmp)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let output = format!(
            "{{\"schema\":1,\"requested_window_us\":30000000,\"trigger_us\":{},\"completed_us\":{now},\"complete_frame_samples\":{},\"discarded_capacity_samples\":{},\"recorded_draws\":{},\"cpu_worker_totals_overlap\":true,\"phases\":{},\"memory_start\":{},\"memory_end\":{}}}\n",
            bench.start,
            bench.samples.rows.len(),
            bench.samples.dropped,
            bench.draws,
            bench.samples.json(),
            bench.memory.json(),
            memory.json()
        );
        std::fs::write(&tmp, output)?;
        std::fs::rename(tmp, &path)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            tracing::info!(target:"starport::bench",path=%path.display(),samples=bench.samples.rows.len(),dropped=bench.samples.dropped,"30-second fixed-scene benchmark saved; scene selection remains the user's responsibility")
        }
        Err(error) => {
            tracing::error!(target:"starport::bench",%error,path=%path.display(),"benchmark summary write failed")
        }
    }
}

pub(crate) fn present() {
    if !enabled() && !bench_enabled() {
        return;
    }
    let now = CLOCK.elapsed().as_micros() as u64;
    let mut row = [0; 10];
    for (i, total) in TOTALS.iter().enumerate() {
        row[i + 1] = total.swap(0, Relaxed);
    }
    let request = TRIGGER.swap(0, Relaxed);
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let previous = state.previous;
    let draws = crate::draw::drawn();
    let frame_draws = draws.saturating_sub(state.previous_draws);
    state.previous_draws = draws;
    state.previous = now;
    state.frame += 1;
    static EVERY: OnceLock<u64> = OnceLock::new();
    let every = *EVERY.get_or_init(|| {
        std::env::var("STARPORT_MEMORY_EVERY_N")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0 && *v <= 600)
            .unwrap_or(30)
    });
    if state.frame == 1 || state.frame.is_multiple_of(every) || request != 0 {
        state.memory = Memory::capture();
        state.memory_frame = state.frame;
    }
    if previous != 0 {
        row[0] = now.saturating_sub(previous);
        state.interval.push(row);
    }
    if request != 0 {
        tracing::info!(target:"starport::bench",trigger_us=request,"F8 benchmark started/restarted; boundary intervals excluded, 30-second window");
        state.bench = Some(Bench {
            start: request,
            samples: Samples::default(),
            memory: state.memory,
            draws: 0,
        });
    }
    if let Some(bench) = state.bench.as_mut()
        && complete_interval(previous, now, bench.start)
    {
        bench.samples.push(row);
        bench.draws += frame_draws;
    }
    let finished = state
        .bench
        .as_ref()
        .is_some_and(|b| now >= b.start.saturating_add(30_000_000));
    let complete = finished.then(|| state.bench.take()).flatten();
    let log = enabled() && now.saturating_sub(state.last_log) >= 2_000_000;
    let interval = if log {
        state.last_log = now;
        Some(std::mem::take(&mut state.interval))
    } else {
        None
    };
    let histogram = enabled() && now.saturating_sub(state.last_histogram) >= 10_000_000;
    if histogram {
        state.last_histogram = now;
    }
    let memory = state.memory;
    let memory_frame = state.memory_frame;
    drop(state);
    if let Some(interval) = interval {
        tracing::info!(target:"starport::subphases",samples=interval.rows.len(),dropped=interval.dropped,stats=%interval.json(),
            "per-present median/p95 us; subphases include recording and overlapping worker CPU time, not disjoint preparing wall time");
        tracing::info!(target:"starport::memory",snapshot=%memory.json(),every_n_frames=every,memory_sample_frame=memory_frame,
            "tracked native allocation lifetimes (main allocators/hosted textures); descriptor stride estimates; driver PSO bytes unavailable; DXGI includes untracked internal/swapchain allocations");
    }
    if histogram {
        tracing::info!(target:"starport::texture_histogram",bytes=?crate::allocstat::histogram(),
        "host mip0 dimension buckets <=64,128,256,512,1024,2048,4096,larger; BC first then uncompressed; allocation bytes, not residency");
    }
    if let Some(bench) = complete {
        write_bench(bench, Memory::capture(), now);
    }
}

fn complete_interval(previous: u64, now: u64, start: u64) -> bool {
    previous >= start && now >= previous && now <= start.saturating_add(30_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benchmark_excludes_partial_trigger_and_end_intervals() {
        assert!(!complete_interval(999, 2000, 1000));
        assert!(complete_interval(1000, 2000, 1000));
        assert!(complete_interval(29_001_000, 30_001_000, 1000));
        assert!(!complete_interval(30_001_000, 31_001_000, 1000));
        assert!(!complete_interval(2000, 1999, 1000));
    }

    #[test]
    fn median_p95_and_bounded_capture_do_not_invent_samples() {
        let mut samples = Samples::default();
        assert_eq!(samples.stats(0), (0, 0));
        for n in 1..=100 {
            samples.push([n; 10]);
        }
        assert_eq!(samples.stats(0), (50, 95));
        for _ in 0..CAP {
            samples.push([200; 10]);
        }
        assert_eq!(samples.rows.len(), CAP);
        assert_eq!(samples.dropped, 100);
        assert_eq!(samples.stats(0), (200, 200));
        assert!(
            samples
                .json()
                .contains("\"merge_scan\":{\"median_us\":200,\"p95_us\":200}")
        );
    }
}
