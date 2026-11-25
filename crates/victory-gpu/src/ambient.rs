// SPDX-License-Identifier: GPL-3.0-or-later
// Adapted from Belobug/VIctory-Belobug-AMD at
// 71652a2c15a0da68cd801890e1f71109670f9ca5 (ambient.rs).
// Starport: allocation-owned immutable descriptors, shape/capability checks.
//! A least amount of indirect light.
//!
//! Grand Theft Auto VI's indirect diffuse light comes from probes it fills
//! by tracing rays, which do not run here yet: its program that resolves
//! them (`CS_ScreenProbes_TemporalResolve_*`) leaves next to nothing, and
//! whatever the sun does not reach directly is black. Right after that
//! program, its output is replaced by a constant (it used to be a floor
//! under what the program wrote, but that is garbage: see the shader). The
//! constant is the same everywhere: the light in these images does not go with the title's exposure
//! (well-lit surfaces hold about the same in a shot at -5.5 EV and in one at
//! -9.75). The title's own lighting then applies it with the surfaces'
//! colors. Raised, not added: the result is the next frame's history.
//!
//! `VICTORY_AMBIENT=<level>` sets it (0.6 by default, 0 for none); `off`
//! keeps the title's own output.
//! shortcut: the same light everywhere, from everywhere; gone once rays run.

use std::sync::OnceLock;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::s;

use crate::host::{HostList, host};
use crate::object::This;

/// The programs it follows, by the start of their name.
pub(crate) const PROGRAM: &[u8] = b"CS_ScreenProbes_TemporalResolve";
/// Where they write the indirect diffuse light (R9G9B9E5 as 32-bit
/// integers): root parameter and place in its table.
pub(crate) const LIGHT: (u8, u32) = (1, 176);

/// 0.6 looked balanced in run a2. `VICTORY_AMBIENT=off` leaves what the
/// title's resolve wrote; 0 is no indirect diffuse light at all.
const DEFAULT: f32 = 0.6;

/// The floor; `None` if off.
pub(crate) fn level() -> Option<f32> {
    static LEVEL: OnceLock<f32> = OnceLock::new();
    let level = *LEVEL.get_or_init(|| {
        let set = std::env::var("VICTORY_AMBIENT").unwrap_or_default();
        if set.trim() == "off" {
            return -1.0;
        }
        set.trim().parse().ok().filter(|l: &f32| l.is_finite() && *l >= 0.0).unwrap_or(DEFAULT)
    });
    (level >= 0.0).then_some(level)
}

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=4, b0), DescriptorTable(UAV(u0))"
cbuffer Floor : register(b0) { float level; float3 tint; };
RWTexture2D<uint> light : register(u0);
float3 unpack(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float most = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(most, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (most * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void main(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    light.GetDimensions(w, h);
    if (at.x >= w || at.y >= h) return;
    // What the resolve left is not kept: without rays its inputs are never
    // written and it holds whatever their memory did (bright magenta on
    // translucent surfaces, blocks of colours indoors: run v6, 2026-11-25).
    light[at.xy] = pack(tint * level);
}
"#;

// No wrapping descriptor ring: each eligible allocation owns one immutable view.
struct Pass {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn make(device: &ID3D12Device) -> windows::core::Result<Pass> {
    let mut support = D3D12_FEATURE_DATA_FORMAT_SUPPORT { Format: DXGI_FORMAT_R32_UINT, ..Default::default() };
    // SAFETY: a feature query with a correctly sized output.
    unsafe { device.CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT,
        (&raw mut support).cast(), size_of_val(&support) as u32)?; }
    if support.Support2.0 & D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE.0 == 0 {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x80004001u32 as i32)));
    }
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and names live through the call; out pointers ours.
    let compiled = unsafe {
        D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            s!("ambient"),
            None,
            None,
            s!("main"),
            s!("cs_5_1"),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    let text = |b: &ID3DBlob| {
        // SAFETY: the blob's own bytes.
        unsafe { std::slice::from_raw_parts(b.GetBufferPointer().cast::<u8>(), b.GetBufferSize()) }
            .to_vec()
    };
    if let Err(e) = compiled {
        if let Some(errors) = errors {
            tracing::warn!("ambient light: {}", String::from_utf8_lossy(&text(&errors)));
        }
        return Err(e);
    }
    let code = text(&code.ok_or_else(windows::core::Error::empty)?);
    // SAFETY: the root signature is inside the shader; plain creations.
    unsafe {
        let root: ID3D12RootSignature = device.CreateRootSignature(0, &code)?;
        let pso = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: code.as_ptr().cast(),
                BytecodeLength: code.len(),
            },
            ..Default::default()
        })?;
        Ok(Pass { root, pso })
    }
}

fn uav_barrier(list: &ID3D12GraphicsCommandList) {
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        ..Default::default()
    };
    // SAFETY: an open list; the barrier names no resource.
    unsafe { list.ResourceBarrier(&[barrier]) };
}

/// Replace the resolved indirect term with a constant. Returns whether a
/// helper dispatch was recorded; the caller must restore guest bindings.
pub(crate) fn raise(list: &HostList, resource: This, view: &D3D12_UNORDERED_ACCESS_VIEW_DESC) -> bool {
    let (Some(level), Some(host)) = (level(), host()) else {
        return false;
    };
    let Some(copy) = crate::resource::host_copy(resource) else {
        return false;
    };
    let light = &copy.texture;
    // SAFETY: a live allocation; the selected view's union is TEXTURE2D
    // before its fields are read below.
    let d = unsafe { light.GetDesc() };
    if !eligible(&d, view) {
        static UNSUPPORTED: std::sync::Once = std::sync::Once::new();
        UNSUPPORTED.call_once(|| tracing::warn!(width=d.Width, height=d.Height,
            format=d.Format.0, layers=d.DepthOrArraySize, view_format=view.Format.0,
            view_dimension=view.ViewDimension.0,
            "ambient fallback skipped: unsupported output shape or view"));
        return false;
    }
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    let Some(pass) = PASS
        .get_or_init(|| {
            make(&host.device)
                .inspect_err(|e| tracing::warn!(error = %e, "no ambient light"))
                .ok()
        })
        .as_ref()
    else {
        return false;
    };
    static ONCE: std::sync::Once = std::sync::Once::new();
    if d.Format != DXGI_FORMAT_R32_UINT {
        ONCE.call_once(|| tracing::warn!(format = d.Format.0, "no ambient light: another format"));
        return false;
    }
    ONCE.call_once(|| {
        tracing::info!(level, width = d.Width, height = d.Height, "Starport ambient fallback enabled (constant indirect light; rays remain unavailable)");
    });
    let Some(views) = copy.ambient_view.get_or_init(|| {
        let created = unsafe {
            host.device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 1, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, NodeMask: 0,
            })
        };
        match created {
            Ok(heap) => {
                // SAFETY: one new slot; immutable for this allocation's lifetime.
                unsafe { host.device.CreateUnorderedAccessView(light, None::<&ID3D12Resource>,
                    Some(view), heap.GetCPUDescriptorHandleForHeapStart()); }
                Some(heap)
            }
            Err(e) => { tracing::warn!(error=%e, "ambient descriptor allocation failed"); None }
        }
    }).as_ref() else { return false; };
    list.keep(views);
    crate::list::ensure_state(list, resource, 0x8);
    list.keep(light);
    let l = &list.list;
    uav_barrier(l);
    // SAFETY: an open list; a live texture the list keeps, in the
    // unordered-access state as the program before left it; a view in a slot
    // no list in flight reads.
    unsafe {
        let gpu = views.GetGPUDescriptorHandleForHeapStart();
        l.SetDescriptorHeaps(&[Some(views.clone())]);
        l.SetComputeRootSignature(&pass.root);
        l.SetPipelineState(&pass.pso);
        let constants = [level, 1.0f32, 0.98, 0.94];
        l.SetComputeRoot32BitConstants(0, 4, constants.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(
            1,
            D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: gpu.ptr,
            },
        );
        l.Dispatch((d.Width as u32).div_ceil(8), d.Height.div_ceil(8), 1);
    }
    uav_barrier(l);
    true
}

fn eligible(d: &D3D12_RESOURCE_DESC, v: &D3D12_UNORDERED_ACCESS_VIEW_DESC) -> bool {
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.Format != DXGI_FORMAT_R32_UINT || d.DepthOrArraySize != 1
        || d.Width == 0 || d.Width > u64::from(u32::MAX) || d.Height == 0
        || d.SampleDesc.Count != 1 || d.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 == 0
        || v.Format != DXGI_FORMAT_R32_UINT || v.ViewDimension != D3D12_UAV_DIMENSION_TEXTURE2D {
        return false;
    }
    // SAFETY: dimension checked above. One immutable descriptor per allocation.
    let t = unsafe { v.Anonymous.Texture2D };
    t.MipSlice == 0 && t.PlaneSlice == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_rejects_other_planes_mips_arrays_and_formats() {
        let mut d = D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 16, Height: 16, DepthOrArraySize: 1, MipLevels: 1,
            Format: DXGI_FORMAT_R32_UINT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, ..Default::default() };
        let mut v = D3D12_UNORDERED_ACCESS_VIEW_DESC { Format: DXGI_FORMAT_R32_UINT,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D, ..Default::default() };
        assert!(eligible(&d, &v));
        v.Anonymous.Texture2D.MipSlice = 1;
        assert!(!eligible(&d, &v));
        v.Anonymous.Texture2D.MipSlice = 0;
        v.Anonymous.Texture2D.PlaneSlice = 1;
        assert!(!eligible(&d, &v));
        v.Anonymous.Texture2D.PlaneSlice = 0;
        d.DepthOrArraySize = 6;
        assert!(!eligible(&d, &v));
        d.DepthOrArraySize = 1;
        d.Format = DXGI_FORMAT_R32_FLOAT;
        assert!(!eligible(&d, &v));
    }

    /// The pass compiles with the system's compiler and the host takes it.
    #[test]
    fn ambient_pass_builds() {
        let Some(host) = crate::host::host() else {
            return;
        };
        super::make(&host.device).expect("the ambient pass");
    }
}
