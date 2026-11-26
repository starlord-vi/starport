// SPDX-License-Identifier: GPL-3.0-or-later
//! Catch CPU copies into descriptor heaps, including inline title stores.
//! Inspired by sync_heaps in Mskh/codec-d--cool_fox-OPTIMIZED at
//! 808342e93a268ded13e67f4aca016214c6d3230d (GPL-3.0-or-later).
//! This implementation uses Starport's non-consuming dirty-page journal,
//! allocation-owned cursors and kernel-read snapshots. No executable hooks.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};

use super::{SHADER_HEAPS, VIEW_TAG, host::HostHeap, increment};

#[derive(Default)]
pub(super) struct Copied {
    records: Vec<[u32; 3]>,
    asked: u64,
    initialized: bool,
    /// A failed kernel read stays pending, even after the journal cursor
    /// advances. Nothing reads a guest page by an unchecked pointer here.
    pending: BTreeSet<usize>,
}

static STATS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_HEAP_SYNC").is_none_or(|v| v != "0"))
}

pub(crate) fn line(frames: f64) {
    let [pages, changes, mirrors, us, full] = STATS.each_ref().map(|n| n.swap(0, Relaxed));
    tracing::info!(target: "starport::descriptors", enabled=on(),
        pages_per_frame=pages as f64 / frames,
        changed_records_per_frame=changes as f64 / frames,
        mirrored_records_per_frame=mirrors as f64 / frames,
        untracked_scans_per_frame=full as f64 / frames,
        ms_per_frame=us as f64 / frames / 1000.0,
        "synchronized raw descriptor copies; timing overlaps preparing/heap work");
}

pub(crate) fn sync_heaps() {
    if !on() {
        return;
    }
    // Clone owners while the registry protects Heap pointers. Neither raw
    // Heap references nor guest memory pointers escape into an unchecked read.
    let jobs: Vec<_> = {
        let heaps = SHADER_HEAPS.lock().unwrap_or_else(|e| e.into_inner());
        heaps
            .iter()
            .filter_map(|&(start, _, at)| {
                // SAFETY: heap removal uses this same registry lock.
                let heap = unsafe { &*(at as *const super::Heap) };
                Some((
                    start,
                    heap.desc.count,
                    increment(heap.desc.kind)?,
                    heap.copied.clone(),
                    heap.host.get()?.as_ref()?.clone(),
                ))
            })
            .collect()
    };
    for (start, count, step, copied, host) in jobs {
        sync(start, count, step, &copied, &host);
    }
}

pub(super) fn sync(
    start: u64,
    count: u32,
    step: u32,
    copied: &Arc<Mutex<Copied>>,
    host: &HostHeap,
) {
    if !on() {
        return;
    }
    if !matches!(step, 16 | 32) {
        return;
    }
    let began = std::time::Instant::now();
    let count = count.min(host.count()) as usize;
    let (start, step) = (start as usize, step as usize);
    let Some(end) = count.checked_mul(step).and_then(|n| start.checked_add(n)) else {
        return;
    };
    if start == end {
        return;
    }
    let mut copied = copied.lock().unwrap_or_else(|e| e.into_inner());
    copied.records.resize(count, [0; 3]);
    let tracked = crate::watch::writes(start, end, true);
    if tracked.is_none() {
        STATS[4].fetch_add(1, Relaxed);
    }
    let page = crate::watch::PAGE;
    if !copied.initialized || tracked.is_none() {
        copied.pending.extend(start / page..end.div_ceil(page));
    }
    if let Some((asked, writes)) = tracked {
        let previous = copied.asked;
        copied.pending.extend(
            writes
                .into_iter()
                .filter_map(|(p, stamp)| (stamp > previous).then_some(p)),
        );
        copied.asked = asked;
    }
    copied.initialized = true;
    // Coalesce consecutive dirty pages into at most 64 KiB reads. A kernel
    // read checks mappings and makes one stable snapshot for comparison.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &p in &copied.pending {
        match runs.last_mut() {
            Some((a, b)) if *b == p && p - *a < 16 => *b += 1,
            _ => runs.push((p, p + 1)),
        }
    }
    let read = crate::work::map(runs.len(), |i| {
        let (a, b) = runs[i];
        let (from, to) = ((a * page).max(start), (b * page).min(end));
        let mut bytes = vec![0; to - from];
        crate::host::read_into(from as *const u8, &mut bytes).then_some((a, b, from, bytes))
    });
    let mut pages = 0;
    let mut changes = 0;
    let mut mirrors = 0;
    for (a, b, from, bytes) in read.into_iter().flatten() {
        pages += b - a;
        for p in a..b {
            copied.pending.remove(&p);
        }
        // Heaps start on descriptor boundaries. Skip a partial record if
        // a future heap layout straddles a kernel page.
        let first = (from - start).div_ceil(step);
        let offset = start + first * step - from;
        for (n, record) in bytes[offset..].chunks_exact(step).enumerate() {
            let index = first + n;
            let record = std::array::from_fn(|i| {
                u32::from_le_bytes(record[4 * i..4 * i + 4].try_into().unwrap())
            });
            let old = &mut copied.records[index];
            if *old == record {
                continue;
            }
            changes += 1;
            if old[0] == VIEW_TAG || record[0] == VIEW_TAG {
                host.mirror_record(index as u32, (start + index * step) as u64, record, false);
                mirrors += 1;
            }
            *old = record;
        }
    }
    STATS[0].fetch_add(pages as u64, Relaxed);
    STATS[1].fetch_add(changes, Relaxed);
    STATS[2].fetch_add(mirrors, Relaxed);
    STATS[3].fetch_add(began.elapsed().as_micros() as u64, Relaxed);
}

#[cfg(test)]
mod tests {
    use crate::descriptor;
    use crate::object::{This, slot};

    /// A material buffer copied with ordinary CPU stores bypasses all
    /// descriptor API hooks. It must still enter the bindless buffer set.
    #[test]
    fn raw_copied_buffer_is_discovered_updated_and_cleared() {
        let title = descriptor::test_view_heap(8);
        let heap = descriptor::heap(title);
        let host = heap.host().expect("hardware descriptor heap required");
        let mut source = [0u32; 8];
        let source_at = source.as_mut_ptr() as u64;
        let mut resource = [0u32; 14];
        resource[0] = 1; // buffer
        resource[4] = 256; // width
        resource[6] = 1; // height
        resource[7] = 1 | (1 << 16); // one layer, one mip
        let mut desc = [0u32; 16];
        desc[1] = 1; // buffer view
        desc[2] = 5768; // default component mapping
        desc[6] = 64; // elements
        desc[7] = 4; // structured stride
        desc[10] = 0x1000; // guest memory
        descriptor::create_placed_srv(std::ptr::null(), &resource, &desc, source_at);
        let target = heap.handle(3).unwrap();
        // SAFETY: both live 32-byte descriptors; target belongs to this heap.
        unsafe { std::ptr::copy_nonoverlapping(source.as_ptr(), target as *mut u32, 8) };
        assert!(!host.buffer_slots().contains(&3));
        heap.sync_copied(host);
        assert!(host.buffer_slots().contains(&3));
        assert_eq!(descriptor::view(target), descriptor::view(source_at));
        assert!(host.buffer_changes().contains(&3));
        heap.sync_copied(host);
        assert!(
            host.buffer_changes().is_empty(),
            "unchanged copies do not churn the queue"
        );
        desc[10] = 0x2000;
        descriptor::create_placed_srv(std::ptr::null(), &resource, &desc, source_at);
        unsafe { std::ptr::copy_nonoverlapping(source.as_ptr(), target as *mut u32, 8) };
        heap.sync_copied(host);
        assert!(host.buffer_changes().contains(&3));
        unsafe { (target as *mut u8).write_bytes(0, 32) };
        heap.sync_copied(host);
        assert!(!host.buffer_slots().contains(&3));
        // The same bypass also applies to streamed texture descriptors.
        // Clearing a deferred texture must retire the waiting queue entry.
        resource[0] = 3;
        resource[4] = 16;
        resource[6] = 16;
        descriptor::create_placed_srv(std::ptr::null(), &resource, &desc, source_at);
        unsafe { std::ptr::copy_nonoverlapping(source.as_ptr(), target as *mut u32, 8) };
        heap.sync_copied(host);
        assert!(
            host.texture_slots_due(u64::MAX, 8)
                .0
                .iter()
                .any(|&(i, _)| i == 3)
        );
        unsafe { (target as *mut u8).write_bytes(0, 32) };
        heap.sync_copied(host);
        assert!(
            !host
                .texture_slots_due(u64::MAX, 8)
                .0
                .iter()
                .any(|&(i, _)| i == 3)
        );
        slot::<extern "system" fn(This) -> u32>(title, 0x10)(title);
    }

    #[test]
    fn unreadable_descriptor_pages_remain_pending() {
        let title = descriptor::test_view_heap(8);
        let heap = descriptor::heap(title);
        let host = heap.host().expect("hardware descriptor heap required");
        let copied = Default::default();
        super::sync(0x1000, 8, 32, &copied, host);
        assert_eq!(copied.lock().unwrap().pending.len(), 1);
        super::sync(0x1000, 8, 32, &copied, host);
        assert_eq!(copied.lock().unwrap().pending.len(), 1);
        slot::<extern "system" fn(This) -> u32>(title, 0x10)(title);
    }
}
