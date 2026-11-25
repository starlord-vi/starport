//! Fence-aware residency for CPU-backed streaming textures. Eviction keeps
//! resource addresses and descriptors valid; every submitted list restores
//! and pins its explicit textures and its heaps' bindless textures first.

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
}

static TEXTURES: LazyLock<Mutex<HashMap<usize, Entry>>> = LazyLock::new(Mutex::default);
static EVICTED: AtomicU64 = AtomicU64::new(0);
static EVICTED_BYTES: AtomicU64 = AtomicU64::new(0);
static RESTORED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn track(texture: &std::sync::Arc<HostTexture>, guest: usize, bytes: u64) {
    TEXTURES.lock().unwrap_or_else(|e| e.into_inner()).insert(
        texture.texture.as_raw() as usize,
        Entry {
            texture: std::sync::Arc::downgrade(texture),
            guest,
            bytes,
            last_submission: 0,
            resident: true,
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
        resident(entry, resource);
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
        resident(entry, &texture.texture);
        entry.last_submission = entry.last_submission.max(submission);
        texture
            .last_used_frame
            .store(crate::frame::current(), Relaxed);
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

pub(crate) fn sweep() {
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
    fn eviction_waits_for_both_age_and_completed_fence() {
        assert!(!cold(10, 99, 4, 4));
        assert!(!cold(10, 100, 5, 4));
        assert!(cold(10, 100, 5, 5));
        assert!(!cold(100, 10, 0, 0));
    }
}
