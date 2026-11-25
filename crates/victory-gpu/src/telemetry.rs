//! Bounded resource attribution. No resource contents are retained or logged.
//! Unique means allocation-offset coverage, not deduplicated pixel contents.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_TELEMETRY").is_none_or(|v| v != "0"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Reason {
    FirstUpload,
    DirtyReupload,
    MipRebase,
    DemotionReupload,
}

impl Reason {
    pub(crate) fn initial(skip: u32, demoted: bool) -> Self {
        if demoted {
            Self::DemotionReupload
        } else if skip > 0 {
            Self::MipRebase
        } else {
            Self::FirstUpload
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::FirstUpload => "first-upload",
            Self::DirtyReupload => "dirty-reupload",
            Self::MipRebase => "mip-rebase",
            Self::DemotionReupload => "demotion-reupload",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Shape {
    pub(crate) guest: u64,
    pub(crate) width: u64,
    pub(crate) height: u32,
    pub(crate) layers: u16,
    pub(crate) format: u32,
    pub(crate) kind: &'static str,
}

pub(crate) struct Resource {
    id: u64,
    shape: Shape,
    covered: Mutex<Vec<(u64, u64)>>,
}

impl Resource {
    pub(crate) fn new(shape: Shape) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Relaxed),
            shape,
            covered: Mutex::default(),
        }
    }

    pub(crate) fn upload(&self, offset: u64, bytes: u64, first_reason: Reason) {
        if !enabled() || bytes == 0 {
            return;
        }
        let mut covered = self.covered.lock().unwrap_or_else(|e| e.into_inner());
        let unique = cover(&mut covered, offset, offset.saturating_add(bytes));
        drop(covered);
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.unique += unique;
        state.total += bytes;
        // A partly new range is attributed byte-for-byte, rather than calling
        // a whole pool a first upload after one newly touched page.
        for (reason, amount) in [
            (first_reason, unique),
            (Reason::DirtyReupload, bytes - unique),
        ] {
            state.frame.reasons[reason as usize] += amount;
            if amount != 0 {
                state
                    .frame
                    .add(false, self.id, reason.name(), self.shape, amount);
            }
        }
    }

    pub(crate) fn readback(&self, bytes: u64, reason: &'static str) {
        if enabled() && bytes != 0 {
            STATE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .frame
                .add(true, self.id, reason, self.shape, bytes);
        }
    }
}

/// Union insertion returns only previously unseen bytes. Adjacent ranges merge.
fn cover(ranges: &mut Vec<(u64, u64)>, from: u64, to: u64) -> u64 {
    if from >= to {
        return 0;
    }
    if ranges.iter().any(|&(a, b)| a <= from && to <= b) {
        return 0;
    }
    let overlap: u64 = ranges
        .iter()
        .map(|&(a, b)| b.min(to).saturating_sub(a.max(from)))
        .sum();
    ranges.push((from, to));
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    for &(a, b) in ranges.iter() {
        if let Some(last) = merged.last_mut()
            && a <= last.1
        {
            last.1 = last.1.max(b);
        } else {
            merged.push((a, b));
        }
    }
    *ranges = merged;
    (to - from).saturating_sub(overlap)
}

#[derive(Default)]
struct Frame {
    resources: BTreeMap<(bool, u64, &'static str), (Shape, u64)>,
    reasons: [u64; 4],
    overflow_bytes: u64,
    refused: BTreeMap<(String, bool), u64>,
    ambient_hits: u64,
}

impl Frame {
    fn add(&mut self, back: bool, id: u64, reason: &'static str, shape: Shape, bytes: u64) {
        let key = (back, id, reason);
        if self.resources.len() < 4096 || self.resources.contains_key(&key) {
            self.resources.entry(key).or_insert((shape, 0)).1 += bytes;
        } else {
            self.overflow_bytes += bytes;
        }
    }
}

struct State {
    frame: Frame,
    total: u64,
    unique: u64,
    last_log: Instant,
    frames: u64,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        frame: Frame::default(),
        total: 0,
        unique: 0,
        last_log: Instant::now(),
        frames: 0,
    })
});
static PHASES: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

pub(crate) fn phase(which: usize, us: u64) {
    crate::perf::phase(which, us);
    if enabled() {
        PHASES[which].fetch_add(us, Relaxed);
    }
}

pub(crate) fn refused(hash: &str, indirect: bool) {
    if enabled() {
        *STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frame
            .refused
            .entry((hash.to_owned(), indirect))
            .or_default() += 1;
    }
}

pub(crate) fn ambient(hit: bool) {
    if enabled() && hit {
        STATE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frame
            .ambient_hits += 1;
    }
}

pub(crate) fn present(flights: usize, pending_frames: usize) {
    crate::ambient_trace::present();
    crate::perf::present();
    if !enabled() {
        return;
    }
    let phases = PHASES.each_ref().map(|p| p.swap(0, Relaxed));
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let frame = std::mem::take(&mut state.frame);
    state.frames += 1;
    if state.last_log.elapsed() < Duration::from_secs(2) {
        return;
    }
    state.last_log = Instant::now();
    let frames = std::mem::take(&mut state.frames);
    let (total, unique) = (state.total, state.unique);
    drop(state);
    tracing::info!(target: "starport::telemetry", sample_frame=crate::frame::current(), frames_since_log=frames,
        preparing_us=phases[0], cpu_fence_wait_us=phases[1], copy_back_us=phases[2], flights, frames_in_flight=pending_frames, frame_limit=crate::frames::limit(),
        cumulative_upload_bytes=total, cumulative_unique_allocation_bytes=unique,
        cumulative_repeat_bytes=total.saturating_sub(unique), first_upload_bytes=frame.reasons[0],
        dirty_reupload_bytes=frame.reasons[1], mip_rebase_bytes=frame.reasons[2], demotion_reupload_bytes=frame.reasons[3],
        overflow_attribution_bytes=frame.overflow_bytes, ambient_hits=frame.ambient_hits,
        live_texture_bytes=crate::resource::HOST_TEXTURES[1].load(Relaxed),
        waiting_texture_slots=crate::draw::HEAP_TEXTURES_WAITING.load(Relaxed),
        waiting_due_slots=crate::draw::HEAP_TEXTURES_QUEUE[1].load(Relaxed),
        waiting_unmapped_slots=crate::draw::HEAP_TEXTURES_QUEUE[2].load(Relaxed),
        waiting_partial_slots=crate::draw::HEAP_TEXTURES_QUEUE[3].load(Relaxed),
        evicted_textures=crate::residency::counters()[0], patched_descriptors=crate::descriptor::host::patched_count(),
        cumulative_fallback_descriptor_writes=crate::fallback::writes(),
        eviction_patched_descriptors=crate::residency::details()[0],
        eviction_pinned_candidates=crate::residency::details()[1],
        overwrite_invalidations=crate::residency::details()[2],
        "sampled presented-frame attribution; phases are CPU wall times, recording may precede submission");
    for back in [false, true] {
        let mut resources: Vec<_> = frame
            .resources
            .iter()
            .filter(|(k, _)| k.0 == back)
            .collect();
        resources.sort_unstable_by_key(|(_, (_, bytes))| std::cmp::Reverse(*bytes));
        for ((_, id, reason), (s, bytes)) in resources.into_iter().take(20) {
            tracing::info!(target: "starport::resource", sample_frame=crate::frame::current(), readback=back,
                id, guest=s.guest, kind=s.kind, width=s.width, height=s.height, layers=s.layers,
                format=s.format, bytes, reason, "resource attribution");
        }
    }
    for ((hash, indirect), attempts) in frame.refused {
        tracing::info!(target: "starport::refused", sample_frame=crate::frame::current(), hash, attempts,indirect,
            "guarded compute dispatch recording attempts; not executed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unique_ranges_handle_overlap_gaps_and_adjacency() {
        let mut spans = Vec::new();
        assert_eq!(cover(&mut spans, 4, 8), 4);
        assert_eq!(cover(&mut spans, 0, 4), 4);
        assert_eq!(cover(&mut spans, 2, 6), 0);
        assert_eq!(cover(&mut spans, 10, 12), 2);
        assert_eq!(cover(&mut spans, 6, 11), 2);
        assert_eq!(spans, [(0, 12)]);
        assert_eq!(cover(&mut spans, 1, 1), 0);
    }
    #[test]
    fn attribution_is_bounded_but_does_not_hide_overflow() {
        let mut frame = Frame::default();
        let s = Shape {
            guest: 0,
            width: 8,
            height: 1,
            layers: 1,
            format: 0,
            kind: "buffer",
        };
        for id in 0..4200 {
            frame.add(false, id, "first-upload", s, 8);
        }
        assert_eq!(frame.resources.len(), 4096);
        assert_eq!(frame.overflow_bytes, 104 * 8);
    }
}
