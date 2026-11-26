//! One bounded CPU pool for scan/copy/hash work. It never submits GPU work.
use rayon::prelude::*;
use std::sync::OnceLock;

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_PARALLEL_WORK").is_none_or(|v| v != "0"))
}
fn count(value: Option<&str>, logical: usize) -> usize {
    value
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or_else(|| logical.saturating_sub(4).max(1))
        .clamp(1, logical.max(1))
}
pub(crate) fn threads() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| {
        let logical=std::thread::available_parallelism().map_or(1,|n|n.get());
        let n=count(std::env::var("STARPORT_WORKER_THREADS").ok().as_deref(),logical);
        tracing::info!(target:"starport::workers",logical,workers=n,parallel=enabled(),"bounded scan/hash workers; 1 runs inline, PARALLEL_WORK=0 restores legacy scheduling");n
    })
}
fn pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads())
            .thread_name(|i| format!("starport-worker-{i}"))
            .build()
            .unwrap_or_else(|error| crate::host::fail(format_args!("CPU worker pool: {error}")))
    })
}
pub(crate) fn run(jobs: Vec<Box<dyn FnOnce() + Send>>) {
    if jobs.len() < 2 || threads() == 1 {
        for job in jobs {
            job();
        }
        return;
    }
    pool().install(|| jobs.into_par_iter().for_each(|job| job()));
}
pub(crate) fn map<T: Send>(count: usize, apply: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    if !enabled() || threads() == 1 || count < 2 {
        return (0..count).map(apply).collect();
    }
    pool().install(|| (0..count).into_par_iter().map(apply).collect())
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ContentHash {
    Blake3(blake3::Hash),
    Xxh3(u128),
}
pub(crate) fn hash(bytes: &[u8]) -> ContentHash {
    let _timer = crate::perf::time(crate::perf::Part::Hashing);
    static FAST: OnceLock<bool> = OnceLock::new();
    if *FAST.get_or_init(|| std::env::var_os("STARPORT_XXH3").is_none_or(|v| v != "0")) {
        ContentHash::Xxh3(xxhash_rust::xxh3::xxh3_128(bytes))
    } else {
        ContentHash::Blake3(blake3::hash(bytes))
    }
}
pub(crate) fn retained_reads() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_RETAINED_MIP_READS").is_none_or(|v| v != "0"))
}

/// Copy live, disjoint CPU ranges. The caller keeps both allocations alive,
/// prevents concurrent writes, and permits writes to the destination until
/// this function returns. GPU work must have completed before this call.
pub(crate) unsafe fn copy(src: *const u8, dst: *mut u8, len: usize) {
    const CHUNK: usize = 1 << 20;
    if threads() == 1 || len < CHUNK {
        // SAFETY: the caller's live, nonoverlapping ranges.
        unsafe { std::ptr::copy_nonoverlapping(src, dst, len) };
        return;
    }
    let (src, dst) = (src as usize, dst as usize);
    pool().install(|| {
        (0..len.div_ceil(CHUNK)).into_par_iter().for_each(|i| {
            let at = i * CHUNK;
            let n = CHUNK.min(len - at);
            // SAFETY: each task owns a disjoint part of the caller's ranges.
            unsafe {
                std::ptr::copy_nonoverlapping((src + at) as *const u8, (dst + at) as *mut u8, n)
            };
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thread_budget_leaves_headroom_and_clamps_explicit_values() {
        assert_eq!(count(None, 16), 12);
        assert_eq!(count(None, 4), 1);
        assert_eq!(count(Some("1"), 16), 1);
        assert_eq!(count(Some("99"), 16), 16);
        assert_eq!(count(Some("0"), 16), 1);
        assert_eq!(count(Some("bad"), 16), 12);
    }
    #[test]
    fn bounded_copy_preserves_partial_final_chunk() {
        let source: Vec<u8> = (0..(3 * (1 << 20) + 73)).map(|i| (i % 251) as u8).collect();
        let mut destination = vec![0; source.len()];
        // SAFETY: live disjoint CPU allocations, no concurrent writers.
        unsafe { copy(source.as_ptr(), destination.as_mut_ptr(), source.len()) };
        assert_eq!(source, destination);
    }
    #[test]
    fn parallel_span_hashes_match_serial_hashes_and_isolate_edits() {
        let mut bytes = vec![13u8; 65536];
        let original = map(16, |i| hash(&bytes[i * 4096..(i + 1) * 4096]));
        assert_eq!(
            original,
            (0..16)
                .map(|i| hash(&bytes[i * 4096..(i + 1) * 4096]))
                .collect::<Vec<_>>()
        );
        bytes[3 * 4096 + 22] = 44;
        let changed = map(16, |i| hash(&bytes[i * 4096..(i + 1) * 4096]));
        for i in 0..16 {
            assert_eq!(original[i] == changed[i], i != 3);
        }
    }
    #[test]
    fn panics_return_to_the_caller_and_do_not_break_the_pool() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(vec![
                Box::new(|| panic!("intentional worker regression")),
                Box::new(|| {}),
            ]);
        }));
        assert!(result.is_err());
        assert_eq!(map(8, |i| i * i), [0, 1, 4, 9, 16, 25, 36, 49]);
    }
}
