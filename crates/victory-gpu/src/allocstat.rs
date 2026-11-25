//! Allocation size accounting, independent of residency. A private COM token
//! follows the native object's final Release, including recorded-list clones.
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface};

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Class {
    Texture,
    RenderTarget,
    Buffer,
    Upload,
    Readback,
    DescriptorPayload,
    PipelineCount,
}
static LIVE: [AtomicU64; 7] = [const { AtomicU64::new(0) }; 7];
static HISTOGRAM: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
const TAG: GUID = GUID::from_u128(0xece62a50_55ba_4f40_970d_d29db2971132);

fn bucket(dimension: u64, bc: bool) -> usize {
    let size = match dimension {
        0..=64 => 0,
        65..=128 => 1,
        129..=256 => 2,
        257..=512 => 3,
        513..=1024 => 4,
        1025..=2048 => 5,
        2049..=4096 => 6,
        _ => 7,
    };
    size + if bc { 0 } else { 8 }
}

#[repr(C)]
struct Token {
    vtable: *const IUnknown_Vtbl,
    refs: AtomicU32,
    class: Class,
    bytes: u64,
    bucket: Option<usize>,
}
impl Drop for Token {
    fn drop(&mut self) {
        LIVE[self.class as usize].fetch_sub(self.bytes, Ordering::Relaxed);
        if let Some(bucket) = self.bucket {
            HISTOGRAM[bucket].fetch_sub(self.bytes, Ordering::Relaxed);
        }
    }
}
unsafe extern "system" fn query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() || iid.is_null() {
        return HRESULT(0x80004003u32 as i32);
    }
    // SAFETY: COM arguments; only IUnknown is implemented by this token.
    unsafe {
        *out = std::ptr::null_mut();
        if *iid != IUnknown::IID {
            return HRESULT(0x80004002u32 as i32);
        }
        *out = this;
        add_ref(this);
    }
    HRESULT(0)
}
unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    // SAFETY: every outstanding interface reference owns the live Token.
    unsafe { &*this.cast::<Token>() }
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    // SAFETY: caller relinquishes one live COM reference.
    let left = unsafe { &*this.cast::<Token>() }
        .refs
        .fetch_sub(1, Ordering::Release)
        - 1;
    if left == 0 {
        std::sync::atomic::fence(Ordering::Acquire);
        // SAFETY: last reference, originally allocated by Box::into_raw.
        unsafe { drop(Box::from_raw(this.cast::<Token>())) };
    }
    left
}
static VTABLE: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: query,
    AddRef: add_ref,
    Release: release,
};

fn attach(object: &ID3D12Object, class: Class, bytes: u64, bucket: Option<usize>) {
    if !crate::perf::collecting() {
        return;
    }
    LIVE[class as usize].fetch_add(bytes, Ordering::Relaxed);
    if let Some(bucket) = bucket {
        HISTOGRAM[bucket].fetch_add(bytes, Ordering::Relaxed);
    }
    let token = Box::new(Token {
        vtable: &VTABLE,
        refs: AtomicU32::new(1),
        class,
        bytes,
        bucket,
    });
    // SAFETY: standard IUnknown layout and reference ownership. The D3D12
    // object AddRefs it, then drops it on final destruction/replacement.
    let token = unsafe { IUnknown::from_raw(Box::into_raw(token).cast()) };
    if let Err(error) = unsafe { object.SetPrivateDataInterface(&TAG, &token) } {
        tracing::warn!(%error, "native allocation accounting token rejected");
    }
}

pub(crate) fn resource(device: &ID3D12Device, resource: &ID3D12Resource, class: Option<Class>) {
    if !crate::perf::collecting() {
        return;
    }
    // SAFETY: a live resource's own description and allocation query.
    let desc = unsafe { resource.GetDesc() };
    let bytes = unsafe { device.GetResourceAllocationInfo(0, &[desc]) }.SizeInBytes;
    if bytes == u64::MAX {
        return;
    }
    let texture = desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER;
    let class = class.unwrap_or(if texture {
        if desc.Flags.0
            & (D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET.0
                | D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL.0)
            != 0
        {
            Class::RenderTarget
        } else {
            Class::Texture
        }
    } else {
        Class::Buffer
    });
    let histogram = texture.then(|| {
        bucket(
            desc.Width.max(u64::from(desc.Height)),
            matches!(desc.Format.0, 70..=84 | 94..=99),
        )
    });
    if let Ok(object) = resource.cast() {
        attach(&object, class, bytes, histogram);
    }
}
pub(crate) fn descriptors(device: &ID3D12Device, heap: &ID3D12DescriptorHeap) {
    if !crate::perf::collecting() {
        return;
    }
    // The descriptor address stride is a payload estimate, not driver VRAM.
    let desc = unsafe { heap.GetDesc() };
    let bytes = u64::from(desc.NumDescriptors)
        * u64::from(unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) });
    if let Ok(object) = heap.cast() {
        attach(&object, Class::DescriptorPayload, bytes, None);
    }
}
pub(crate) fn pipeline(pso: &ID3D12PipelineState) {
    if let Ok(object) = pso.cast() {
        attach(&object, Class::PipelineCount, 1, None);
    }
}
pub(crate) fn snapshot() -> [u64; 7] {
    LIVE.each_ref().map(|v| v.load(Ordering::Relaxed))
}
pub(crate) fn histogram() -> [u64; 16] {
    HISTOGRAM.each_ref().map(|v| v.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_lifetime_token_survives_clones_and_leaves_no_counter_after_drop() {
        let host = crate::host::host().unwrap();
        let before = snapshot()[Class::Readback as usize];
        let buffer = host.read_back_buffer(4096).unwrap();
        let during = snapshot()[Class::Readback as usize];
        assert!(during >= before + 4096);
        let clone = buffer.clone();
        drop(buffer);
        assert_eq!(snapshot()[Class::Readback as usize], during);
        drop(clone);
        assert_eq!(snapshot()[Class::Readback as usize], before);
    }
    #[test]
    fn histogram_covers_both_classes_and_large_textures() {
        assert_eq!(bucket(64, true), 0);
        assert_eq!(bucket(512, false), 11);
        assert_eq!(bucket(2048, true), 5);
        assert_eq!(bucket(65536, false), 15);
    }
}
