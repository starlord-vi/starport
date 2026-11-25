//! Descriptor-patched reclamation of CPU-owned streaming shadows. Legacy
//! Evict/MakeResident remains explicitly selectable. Recorded owners pin data.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex, Weak};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

use crate::resource::HostTexture;

struct Entry {
    texture: Weak<HostTexture>,
    guest: usize,
    bytes: u64,
    last_submission: u64,
    resident: bool,
    range: (usize, usize),
    invalidated: bool,
    hard_invalidated: bool,
    target_bytes: u64,
    promotable: bool,
    promotion_retry_ms: u64,
}

static TEXTURES: LazyLock<Mutex<HashMap<usize, Entry>>> = LazyLock::new(Mutex::default);
static EVICTED: AtomicU64 = AtomicU64::new(0);
static EVICTED_BYTES: AtomicU64 = AtomicU64::new(0);
static RESTORED: AtomicU64 = AtomicU64::new(0);
static PATCHES: AtomicU64 = AtomicU64::new(0);
static PINNED: AtomicU64 = AtomicU64::new(0);
static INVALIDATED: AtomicU64 = AtomicU64::new(0);
static CHANGES: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
static REMAPS: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
static POLICY_SKIPS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static BOUNDS: [AtomicU64; 2] = [AtomicU64::new(u64::MAX), AtomicU64::new(0)];

pub(crate) fn legacy() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_LEGACY_RESIDENCY").is_some_and(|v| v == "1"))
}
fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_RESIDENCY").is_none_or(|v| v != "0"))
}
pub(crate) fn target_pct() -> u64 {
    static PCT: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *PCT.get_or_init(|| {
        std::env::var("STARPORT_VRAM_TARGET_PCT")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|n| (50..=95).contains(n))
            .unwrap_or(85)
    })
}
fn v2() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_EVICTOR_V2").is_none_or(|v| v != "0"))
}
fn minimum_bytes() -> u64 {
    static VALUE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("STARPORT_EVICT_MIN_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v <= 4 << 30)
            .unwrap_or(2 << 20)
    })
}
fn minimum_ms() -> u64 {
    static VALUE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("STARPORT_EVICT_MIN_RESIDENCY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v <= 60000)
            .unwrap_or(2000)
    })
}
#[derive(Debug, PartialEq, Eq)]
enum Excluded {
    Uncompressed,
    Small,
    Recent,
    Thrashing,
}
fn policy(
    format: u32,
    bytes: u64,
    age: u64,
    reuploads: u32,
    minimum: u64,
    ms: u64,
) -> Option<Excluded> {
    if !matches!(format,70..=84|94..=99) {
        Some(Excluded::Uncompressed)
    } else if bytes < minimum {
        Some(Excluded::Small)
    } else if age < ms {
        Some(Excluded::Recent)
    } else if reuploads >= 2 {
        Some(Excluded::Thrashing)
    } else {
        None
    }
}
pub(crate) fn policy_skips() -> [u64; 4] {
    POLICY_SKIPS.each_ref().map(|v| v.load(Relaxed))
}
pub(crate) fn note_overwrite(at: usize, len: usize) {
    note_range(at, len, &CHANGES);
}
pub(crate) fn note_remap(at: usize, len: usize) {
    note_range(at, len, &CHANGES);
    note_range(at, len, &REMAPS);
}
fn note_range(at: usize, len: usize, journal: &Mutex<Vec<(usize, usize)>>) {
    if !enabled() || legacy() || len == 0 {
        return;
    }
    let end = at.saturating_add(len);
    if (end as u64) <= BOUNDS[0].load(Relaxed) || at as u64 >= BOUNDS[1].load(Relaxed) {
        return;
    }
    let mut changes = journal.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(last) = changes.last_mut()
        && at <= last.1
        && end >= last.0
    {
        *last = (last.0.min(at), last.1.max(end));
    } else {
        changes.push((at, end));
    }
    if changes.len() > 8192 {
        let first = changes.iter().map(|v| v.0).min().unwrap_or(at);
        let last = changes.iter().map(|v| v.1).max().unwrap_or(end);
        changes.clear();
        changes.push((first, last));
    }
}

pub(crate) fn uploaded(key: usize) {
    let mut held = Vec::new();
    let mut entries = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = entries.get_mut(&key) {
        entry.invalidated = false;
        entry.hard_invalidated = false;
        if let Some(texture) = entry.texture.upgrade() {
            if texture.upload_count.fetch_add(1, Relaxed) > 0 {
                texture.reuploads.fetch_add(1, Relaxed);
            }
            texture
                .last_uploaded_ms
                .store(crate::host::now_ms(), Relaxed);
            texture
                .last_used_frame
                .store(crate::frame::current(), Relaxed);
            held.push(texture);
        }
    }
}

pub(crate) fn track(texture: &std::sync::Arc<HostTexture>, guest: usize, bytes: u64) {
    let target = texture
        .tail
        .then(|| crate::resource::streaming_target(guest as crate::object::This))
        .flatten();
    let target_bytes = target.map_or(bytes, |(_, bytes)| bytes);
    let promotable = target.is_some_and(|(mip, _)| mip < texture.first_mip);
    let range = crate::resource::texture_memory(guest as crate::object::This)
        .map(|(at, desc)| {
            let len = crate::resource::texture_layout(&desc)
                .map_or(0, |t| t.planes[0].layout.size as usize);
            (at as usize, (at as usize).saturating_add(len))
        })
        .unwrap_or((0, 0));
    if range.0 < range.1 {
        BOUNDS[0].fetch_min(range.0 as u64, Relaxed);
        BOUNDS[1].fetch_max(range.1 as u64, Relaxed);
    }
    TEXTURES.lock().unwrap_or_else(|e| e.into_inner()).insert(
        texture.texture.as_raw() as usize,
        Entry {
            texture: std::sync::Arc::downgrade(texture),
            guest,
            bytes,
            last_submission: 0,
            resident: true,
            range,
            invalidated: false,
            hard_invalidated: false,
            target_bytes,
            promotable,
            promotion_retry_ms: 0,
        },
    );
}

fn resident(entry: &mut Entry, resource: &ID3D12Resource) {
    if entry.resident {
        return;
    }
    let host = crate::host::host()
        .unwrap_or_else(|| crate::host::fail(format_args!("no host device for texture residency")));
    let pageable = resource
        .cast::<ID3D12Pageable>()
        .unwrap_or_else(|e| crate::host::fail(format_args!("texture residency interface: {e}")));
    // SAFETY: live committed resource. Synchronous MakeResident completes
    // before commands that name it are submitted. Failure is never hidden.
    unsafe { host.device.MakeResident(&[Some(pageable)]) }.unwrap_or_else(|e| {
        crate::host::fail(format_args!(
            "cannot restore a streaming texture's residency: {e}"
        ))
    });
    entry.resident = true;
    RESTORED.fetch_add(1, Relaxed);
}

/// A descriptor can become visible to a submission already in flight.
pub(crate) fn descriptor_kept(resource: &ID3D12Resource) -> bool {
    let mut held = Vec::new();
    let mut entries = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = entries.get_mut(&(resource.as_raw() as usize)) {
        if legacy() {
            resident(entry, resource);
        }
        entry.last_submission = entry.last_submission.max(crate::host::started_submission());
        if let Some(texture) = entry.texture.upgrade() {
            texture
                .last_used_frame
                .store(crate::frame::current(), Relaxed);
            held.push(texture);
        }
        return true;
    }
    false
}

pub(crate) fn prepare(resources: impl IntoIterator<Item = usize>, submission: u64) {
    let resources: HashSet<usize> = resources.into_iter().collect();
    let mut held = Vec::new();
    let mut entries = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    for key in resources {
        let Some(entry) = entries.get_mut(&key) else {
            continue;
        };
        let Some(texture) = entry.texture.upgrade() else {
            continue;
        };
        if legacy() {
            resident(entry, &texture.texture);
        }
        entry.last_submission = entry.last_submission.max(submission);
        if legacy() {
            texture
                .last_used_frame
                .store(crate::frame::current(), Relaxed);
        }
        held.push(texture);
    }
}

/// Stale descriptors can outlive a title resource. Leave their resource
/// resident when its last Arc goes away; the descriptor's COM reference
/// remains its owner. Never leave an untracked evicted resource behind.
pub(crate) fn forget(resource: &ID3D12Resource) {
    if let Some(mut entry) = TEXTURES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&(resource.as_raw() as usize))
        && crate::host::references(resource) > 1
    {
        resident(&mut entry, resource);
    }
}

fn cold(last_frame: u64, frame: u64, last_submission: u64, finished: u64) -> bool {
    frame.saturating_sub(last_frame) >= 90 && last_submission <= finished
}

fn evict(entry: &mut Entry, resource: &ID3D12Resource) -> windows::core::Result<()> {
    let host = crate::host::host()
        .ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
    let pageable = resource.cast::<ID3D12Pageable>()?;
    // SAFETY: the caller verified that no unfinished submission names it.
    unsafe { host.device.Evict(&[Some(pageable)]) }?;
    entry.resident = false;
    EVICTED.fetch_add(1, Relaxed);
    EVICTED_BYTES.fetch_add(entry.bytes, Relaxed);
    Ok(())
}

#[cfg(test)]
pub(crate) fn eviction_roundtrip(texture: &std::sync::Arc<HostTexture>) {
    let mut entries = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let entry = entries
        .get_mut(&(texture.texture.as_raw() as usize))
        .expect("tracked texture");
    assert!(entry.last_submission <= crate::host::finished_submission());
    evict(entry, &texture.texture).expect("D3D12 eviction");
    assert!(!entry.resident);
    resident(entry, &texture.texture);
    assert!(entry.resident);
}

fn legacy_sweep() {
    let Some(host) = crate::host::host() else {
        return;
    };
    let Some([budget, used, ..]) = host.video_memory() else {
        return;
    };
    if budget == 0 || used < budget / 5 * 4 {
        return;
    }
    // Hysteresis: stop around 72% rather than evicting the same few
    // textures every time the system nudges its budget.
    let mut reclaim = used.saturating_sub(budget / 100 * 72);
    let frame = crate::frame::current();
    let finished = crate::host::finished_submission();
    // Strong references must drop after the registry lock: their destructor
    // unregisters them, including when a title concurrently frees a resource.
    let mut held = Vec::new();
    let mut entries = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    for (&key, entry) in entries.iter() {
        if let Some(texture) = entry.texture.upgrade() {
            held.push((key, texture));
        }
    }
    let mut candidates: Vec<_> = held
        .iter()
        .filter_map(|(key, texture)| {
            let entry = &entries[key];
            let last = texture.last_used_frame.load(Relaxed);
            (entry.resident
                && !texture.tail
                && cold(last, frame, entry.last_submission, finished)
                && !crate::texture::host_wrote(entry.guest))
            .then_some((last, *key, texture))
        })
        .collect();
    candidates.sort_unstable_by_key(|(frame, key, _)| (*frame, *key));
    for (_, key, texture) in candidates {
        if reclaim == 0 {
            break;
        }
        let entry = entries
            .get_mut(&key)
            .expect("candidate from locked residency table");
        // SAFETY: no unfinished submission names this texture. Recorded
        // lists may name it, and prepare restores it before their next run.
        match evict(entry, &texture.texture) {
            Ok(()) => {
                reclaim = reclaim.saturating_sub(entry.bytes);
            }
            Err(error) => {
                tracing::warn!(%error, "streaming texture eviction refused");
                break;
            }
        }
    }
}

pub(crate) fn details() -> [u64; 3] {
    [
        PATCHES.load(Relaxed),
        PINNED.load(Relaxed),
        INVALIDATED.load(Relaxed),
    ]
}
fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}
fn merged_changes(mut changes: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    changes.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for r in changes {
        if let Some(last) = out.last_mut()
            && r.0 <= last.1
        {
            last.1 = last.1.max(r.1);
        } else {
            out.push(r);
        }
    }
    out
}
fn changed(range: (usize, usize), changes: &[(usize, usize)]) -> bool {
    let at = changes.partition_point(|r| r.1 <= range.0);
    changes.get(at).is_some_and(|&r| overlaps(range, r))
}
fn eviction_key(invalidated: bool, last: u64, key: usize) -> (bool, u64, usize) {
    (!invalidated, last, key)
}

pub(crate) fn sweep() {
    promote_tails();
    static LOG: std::sync::Once = std::sync::Once::new();
    LOG.call_once(||tracing::info!(target:"starport::residency",enabled=enabled(),legacy=legacy(),v2=v2(),target_pct=target_pct(),cold_frames=90,min_bytes=minimum_bytes(),min_residency_ms=minimum_ms(),pin_after_reuploads=2,
        "streaming residency policy; recorded owners and unfinished submissions remain pinned"));
    if !enabled() {
        return;
    }
    if legacy() {
        legacy_sweep();
        return;
    }
    // Called under the submission mutex: no new execution can begin.
    if crate::host::started_submission() != crate::host::finished_submission() {
        return;
    }
    let Some(host) = crate::host::host() else {
        return;
    };
    let [budget, used, ..] = host.video_memory().unwrap_or([0; 4]);
    let target = budget / 100 * target_pct();
    let pressure = budget > 0 && used >= target;
    let mut reclaim = if pressure {
        used.saturating_sub(budget / 100 * target_pct().saturating_sub(5))
    } else {
        0
    };
    let changes = merged_changes(std::mem::take(
        &mut *CHANGES.lock().unwrap_or_else(|e| e.into_inner()),
    ));
    let remaps = merged_changes(std::mem::take(
        &mut *REMAPS.lock().unwrap_or_else(|e| e.into_inner()),
    ));
    let now = crate::host::now_ms();
    let frame = crate::frame::current();
    let finished = crate::host::finished_submission();
    let (mut candidates, mut held) = (Vec::new(), Vec::new());
    {
        let mut entries = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
        for (&key, entry) in entries.iter_mut() {
            if changed(entry.range, &remaps) {
                entry.hard_invalidated = true;
            }
            if !entry.invalidated && (entry.hard_invalidated || changed(entry.range, &changes)) {
                entry.invalidated = true;
                INVALIDATED.fetch_add(1, Relaxed);
            }
            let Some(texture) = entry.texture.upgrade() else {
                continue;
            };
            let last = texture.last_used_frame.load(Relaxed);
            let age = now.saturating_sub(
                texture
                    .created_ms
                    .max(texture.last_uploaded_ms.load(Relaxed)),
            );
            let reuploads = texture.reuploads.load(Relaxed);
            let excluded = if v2() && !entry.hard_invalidated {
                policy(
                    texture.format,
                    entry.bytes,
                    age,
                    reuploads,
                    minimum_bytes(),
                    minimum_ms(),
                )
            } else {
                None
            };
            let eligible = if entry.invalidated {
                entry.last_submission <= finished && excluded.is_none()
            } else {
                pressure
                    && !texture.tail
                    && excluded.is_none()
                    && cold(last, frame, entry.last_submission, finished)
            };
            if (entry.invalidated || pressure)
                && let Some(reason) = excluded
            {
                POLICY_SKIPS[reason as usize].fetch_add(1, Relaxed);
            }
            if eligible && std::sync::Arc::strong_count(&texture) == 2 {
                candidates.push((
                    !entry.invalidated,
                    last,
                    key,
                    entry.guest,
                    texture.epoch,
                    entry.bytes,
                    age,
                    reuploads,
                    entry.hard_invalidated,
                    texture,
                ));
            } else {
                if eligible {
                    PINNED.fetch_add(1, Relaxed);
                }
                held.push(texture);
            }
        }
    }
    drop(held);
    candidates.sort_unstable_by_key(|c| eviction_key(!c.0, c.1, c.2));
    let mut processed = 0;
    for (not_invalid, last, key, guest, epoch, bytes, age, reuploads, hard, texture) in candidates {
        if processed >= 64 {
            break;
        }
        if not_invalid && reclaim == 0 {
            continue;
        }
        if texture.last_used_frame.load(Relaxed) != last {
            continue;
        }
        let transaction = if v2() {
            crate::descriptor::host::patch_for_promotion(key, frame)
        } else {
            None
        };
        let patched = if v2() {
            let Some(patch) = transaction.as_ref() else {
                PINNED.fetch_add(1, Relaxed);
                continue;
            };
            patch.count()
        } else {
            let Some(patched) = crate::descriptor::host::patch_cold_resource(key, frame) else {
                PINNED.fetch_add(1, Relaxed);
                continue;
            };
            patched
        };
        PATCHES.fetch_add(patched, Relaxed);
        drop(texture);
        if crate::resource::reclaim_streaming(guest, key, epoch) {
            if let Some(patch) = transaction {
                patch.commit_eviction();
            }
            EVICTED.fetch_add(1, Relaxed);
            EVICTED_BYTES.fetch_add(bytes, Relaxed);
            reclaim = reclaim.saturating_sub(bytes);
            tracing::info!(target:"starport::eviction",guest,bytes,patched,age_ms=age,reuploads,
                reason=if hard {"mapping-generation-change"} else if !not_invalid {"guest-overwrite"} else {"local-budget-pressure"},
                "released backing after fence completion and descriptor patching; no CPU pixel cache retained");
        } else {
            drop(transaction); // failed backing release restores real descriptors
            PINNED.fetch_add(1, Relaxed);
        }
        processed += 1;
    }
}

fn promote_tails() {
    if !crate::progressive::enabled()
        || crate::host::started_submission() != crate::host::finished_submission()
    {
        return;
    }
    // This cap includes sorting, budget queries and unsuccessful attempts, not
    // only successful copies. The final indivisible initialization may overrun.
    struct Quota {
        frame: u64,
        us: u64,
        attempts: u64,
    }
    static QUOTA: Mutex<Quota> = Mutex::new(Quota {
        frame: u64::MAX,
        us: 0,
        attempts: 0,
    });
    let frame = crate::frame::current();
    let mut quota = QUOTA.lock().unwrap_or_else(|e| e.into_inner());
    if quota.frame != frame {
        *quota = Quota {
            frame,
            us: 0,
            attempts: 0,
        };
    }
    if quota.attempts >= 8 || quota.us >= 16000 {
        return;
    }
    let initial_us = quota.us;
    let measured = std::time::Instant::now();
    let now = crate::host::now_ms();
    let candidates = {
        let entries = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
        entries
            .iter()
            .filter_map(|(&key, entry)| {
                if !entry.promotable
                    || entry.invalidated
                    || entry.promotion_retry_ms > now
                    || entry.last_submission > crate::host::finished_submission()
                {
                    return None;
                }
                // Every upgraded Arc is retained until outside the registry lock:
                // dropping the last one there could recurse through forget().
                let texture = entry.texture.upgrade()?;
                Some((
                    std::cmp::Reverse(texture.last_used_frame.load(Relaxed)),
                    entry.target_bytes,
                    key,
                    entry.guest,
                    texture.epoch,
                    texture,
                ))
            })
            .collect::<Vec<_>>()
    };
    let mut candidates = candidates;
    candidates.sort_unstable_by_key(|c| (c.0, c.1, std::cmp::Reverse(c.4)));
    for (_, _, key, guest, epoch, texture) in candidates {
        if quota.attempts >= 8 || initial_us + measured.elapsed().as_micros() as u64 >= 16000 {
            break;
        }
        let Some(bytes) = crate::resource::promotion_bytes(guest, key, epoch) else {
            continue;
        };
        let room = crate::host::host()
            .and_then(|h| h.video_memory())
            .is_some_and(|[budget, used, ..]| {
                budget > 0 && used.saturating_add(bytes) <= budget / 100 * target_pct()
            });
        if !room {
            continue;
        }
        if let Some(entry) = TEXTURES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&key)
        {
            entry.promotion_retry_ms = now + 2000;
        }
        drop(texture);
        quota.attempts += 1;
        if let Some(patch) = crate::descriptor::host::patch_for_promotion(key, frame) {
            crate::resource::promote_tail(guest, key, epoch);
            drop(patch);
        }
    }
    quota.us = initial_us + measured.elapsed().as_micros() as u64;
}

pub(crate) fn counters() -> [u64; 3] {
    [
        EVICTED.load(Relaxed),
        EVICTED_BYTES.load(Relaxed),
        RESTORED.load(Relaxed),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn churn_policy_excludes_ui_small_recent_and_repeated_uploads() {
        assert_eq!(
            policy(28, 4 << 20, 9000, 0, 2 << 20, 2000),
            Some(Excluded::Uncompressed)
        );
        assert_eq!(
            policy(98, 1 << 20, 9000, 0, 2 << 20, 2000),
            Some(Excluded::Small)
        );
        assert_eq!(
            policy(98, 4 << 20, 1999, 0, 2 << 20, 2000),
            Some(Excluded::Recent)
        );
        assert_eq!(
            policy(98, 4 << 20, 9000, 2, 2 << 20, 2000),
            Some(Excluded::Thrashing)
        );
        assert_eq!(policy(98, 4 << 20, 2000, 1, 2 << 20, 2000), None);
        assert_eq!(policy(71, 4096, 0, 0, 0, 0), None);
    }
    #[test]
    fn lru_and_overwrite_priority_are_deterministic() {
        let mut entries = [(false, 12, 4), (false, 2, 7), (true, 20, 1), (false, 2, 3)];
        entries.sort_by_key(|&(invalid, last, key)| eviction_key(invalid, last, key));
        assert_eq!(
            entries,
            [(true, 20, 1), (false, 2, 3), (false, 2, 7), (false, 12, 4)]
        );
        assert!(!overlaps((10, 20), (20, 30)));
        assert!(overlaps((10, 20), (19, 30)));
        let changes = merged_changes(vec![(20, 30), (4, 10), (8, 20), (90, 100)]);
        assert_eq!(changes, [(4, 30), (90, 100)]);
        assert!(changed((8, 12), &changes));
        assert!(!changed((30, 90), &changes));
    }

    #[test]
    fn eviction_waits_for_both_age_and_completed_fence() {
        assert!(!cold(10, 99, 4, 4));
        assert!(!cold(10, 100, 5, 4));
        assert!(cold(10, 100, 5, 5));
        assert!(!cold(100, 10, 0, 0));
    }
}
