//! Small real mip tails use a separate, strictly reserved allocation budget.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static BYTES: AtomicU64 = AtomicU64::new(0);
static TAILS: AtomicU64 = AtomicU64::new(0);
static PROMOTED: AtomicU64 = AtomicU64::new(0);
static UNAVAILABLE: AtomicU64 = AtomicU64::new(0);
pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_PROGRESSIVE_TEXTURES").is_none_or(|v| v != "0"))
        && budget() != 0
}
pub(crate) fn budget() -> u64 {
    static LIMIT: OnceLock<u64> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("STARPORT_TAIL_BUDGET_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v <= 4096)
            .unwrap_or(512)
            << 20
    })
}
/// Smallest four mips, retaining at least three for a large image. A truncated
/// source chain may not contain such a <=64px tail; never invent those pixels.
pub(crate) fn tail_mip(width: u64, height: u32, levels: u32, target: u32) -> Option<u32> {
    if levels == 0 || levels > 32 || target >= levels {
        return None;
    }
    if (width.max(u64::from(height)) >> target).max(1) <= 64 {
        return Some(target);
    }
    let mut first = levels.saturating_sub(4).max(target);
    while (width.max(u64::from(height)) >> first).max(1) > 64 {
        first += 1;
    }
    (first <= levels.saturating_sub(3)).then_some(first)
}

/// A private, serialized initialization queue avoids recursively taking the
/// renderer submission mutex. Full promoted pixels are complete before any
/// descriptor is republished, including lists recorded before the promotion.
pub(crate) fn initialize(
    texture: &windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    pixels: &[u8],
) -> windows::core::Result<()> {
    use std::sync::Mutex;
    use windows::Win32::Foundation::{E_FAIL, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Graphics::Direct3D12::*;
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    struct Initializer {
        queue: ID3D12CommandQueue,
        allocator: ID3D12CommandAllocator,
        list: ID3D12GraphicsCommandList,
        fence: ID3D12Fence,
        event: HANDLE,
        value: u64,
        quarantine: Option<(ID3D12Resource, ID3D12Resource)>,
    }
    // SAFETY: D3D12 objects are free-threaded; all access is under this mutex.
    unsafe impl Send for Initializer {}
    static INIT: OnceLock<Option<Mutex<Initializer>>> = OnceLock::new();
    let host = crate::host::host().ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
    let make = || -> windows::core::Result<Initializer> {
        // SAFETY: private queue, allocator, list and completion event.
        unsafe {
            let queue = host
                .device
                .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default())?;
            let allocator = host
                .device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = host.device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None,
            )?;
            list.Close()?;
            let fence = host.device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let event = CreateEventW(None, false, false, None)?;
            Ok(Initializer {
                queue,
                allocator,
                list,
                fence,
                event,
                value: 0,
                quarantine: None,
            })
        }
    };
    let init=INIT.get_or_init(|| make().map(Mutex::new).map_err(|error|tracing::error!(%error,"promotion initializer unavailable; real tails retained")).ok())
        .as_ref().ok_or_else(||windows::core::Error::from_hresult(E_FAIL))?;
    let mut init = init.lock().unwrap_or_else(|e| e.into_inner());
    if init.quarantine.is_some() {
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    let (footprints, total) = crate::texture::initialization_footprints(texture)
        .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
    if pixels.len() as u64 != total {
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    let (upload, data) = host
        .upload_buffer(total)
        .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
    let _timer = crate::perf::time(crate::perf::Part::UploadRecording);
    // SAFETY: immutable CPU snapshot, owned upload and image, and a private
    // allocator reused only after this queue's previous completion.
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), data, pixels.len());
        init.allocator.Reset()?;
        init.list.Reset(&init.allocator, None)?;
        init.list.ResourceBarrier(&[crate::host::transition(
            texture,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_DEST,
        )]);
        for (i, footprint) in footprints.into_iter().enumerate() {
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(texture),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: i as u32,
                },
            };
            let src = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&upload),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: footprint,
                },
            };
            init.list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
        }
        init.list.ResourceBarrier(&[crate::host::transition(
            texture,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_COMMON,
        )]);
        init.list.Close()?;
        init.value += 1;
        init.fence.SetEventOnCompletion(init.value, init.event)?;
        init.queue
            .ExecuteCommandLists(&[Some(windows::core::Interface::cast(&init.list)?)]);
        let complete = init.queue.Signal(&init.fence, init.value).is_ok()
            && WaitForSingleObject(init.event, 30000) == WAIT_OBJECT_0
            && init.fence.GetCompletedValue() == init.value;
        if !complete {
            // The queue/list/allocator/event and uncertain resources remain
            // alive for the process; no further initialization can accumulate.
            init.quarantine = Some((texture.clone(), upload));
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
    }
    Ok(())
}
pub(crate) struct TailLease(u64);
impl Drop for TailLease {
    fn drop(&mut self) {
        BYTES.fetch_sub(self.0, Relaxed);
        TAILS.fetch_sub(1, Relaxed);
    }
}
pub(crate) fn reserve(bytes: u64) -> Option<TailLease> {
    BYTES
        .fetch_update(Relaxed, Relaxed, |used| {
            used.checked_add(bytes).filter(|next| *next <= budget())
        })
        .ok()?;
    TAILS.fetch_add(1, Relaxed);
    Some(TailLease(bytes))
}
pub(crate) fn unavailable() {
    UNAVAILABLE.fetch_add(1, Relaxed);
}
pub(crate) fn promoted() {
    PROMOTED.fetch_add(1, Relaxed);
}
pub(crate) fn counters() -> [u64; 4] {
    [
        BYTES.load(Relaxed),
        TAILS.load(Relaxed),
        PROMOTED.load(Relaxed),
        UNAVAILABLE.load(Relaxed),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tails_respect_source_chain_and_target_and_never_synthesize_mips() {
        assert_eq!(tail_mip(4096, 4096, 13, 2), Some(9));
        assert_eq!(tail_mip(4096, 4096, 9, 0), Some(6));
        assert_eq!(tail_mip(4096, 4096, 8, 0), None);
        assert_eq!(tail_mip(64, 32, 1, 0), Some(0));
        assert_eq!(tail_mip(4096, 4096, 13, 10), Some(10));
        assert_eq!(tail_mip(1, 1, 0, 0), None);
    }
    #[test]
    fn reservation_failure_leaves_accounting_unchanged() {
        let before = counters();
        assert!(reserve(budget().saturating_add(1)).is_none());
        assert_eq!(counters(), before);
        let lease = reserve(4096).unwrap();
        assert_eq!(counters()[0], before[0] + 4096);
        drop(lease);
        assert_eq!(counters(), before);
    }
}
