//! Downscale oversized title outputs without an invalid mismatched CopyResource.
//! Each recorded blit owns its descriptors until its completion fence/reset.
use crate::host::HostList;
#[cfg(test)]
use crate::host::transition;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCompile, ID3DBlob,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{Result, s};

const SOURCE: &str = r#"
#define ROOT "DescriptorTable(SRV(t0)),RootConstants(num32BitConstants=2,b0),StaticSampler(s0,filter=FILTER_MIN_MAG_MIP_LINEAR,addressU=TEXTURE_ADDRESS_CLAMP,addressV=TEXTURE_ADDRESS_CLAMP,addressW=TEXTURE_ADDRESS_CLAMP)"
Texture2D<float4> image : register(t0);
SamplerState linear_clamp : register(s0);
cbuffer Size : register(b0) { uint width; uint height; }
[RootSignature(ROOT)]
float4 vs(uint id:SV_VertexID):SV_Position {
    float2 uv=float2((id<<1)&2,id&2);
    return float4(uv*float2(2,-2)+float2(-1,1),0,1);
}
[RootSignature(ROOT)]
float4 ps(float4 p:SV_Position):SV_Target {
    return image.SampleLevel(linear_clamp,p.xy/float2(width,height),0);
}
"#;

const COMPOSE_SOURCE: &str = r#"
#define ROOT "DescriptorTable(SRV(t0,numDescriptors=2)),RootConstants(num32BitConstants=2,b0),StaticSampler(s0,filter=FILTER_MIN_MAG_MIP_LINEAR,addressU=TEXTURE_ADDRESS_CLAMP,addressV=TEXTURE_ADDRESS_CLAMP,addressW=TEXTURE_ADDRESS_CLAMP)"
Texture2D<float4> scene : register(t0);
Texture2D<float4> overlay : register(t1);
SamplerState linear_clamp : register(s0);
cbuffer Size : register(b0) { uint width; uint height; }
[RootSignature(ROOT)]
float4 vs(uint id:SV_VertexID):SV_Position {
    float2 uv=float2((id<<1)&2,id&2);
    return float4(uv*float2(2,-2)+float2(-1,1),0,1);
}
[RootSignature(ROOT)]
float4 ps(float4 p:SV_Position):SV_Target {
    float2 uv=p.xy/float2(width,height);
    float4 base=scene.SampleLevel(linear_clamp,uv,0);
    float4 over=overlay.SampleLevel(linear_clamp,uv,0);
    float alpha=saturate(over.a);
    return float4(saturate(over.rgb+base.rgb*(1-alpha)),1);
}
"#;

struct Pass {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
}
// SAFETY: immutable, free-threaded D3D12 objects.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}
static PASSES: LazyLock<Mutex<BTreeMap<i32, Arc<Pass>>>> = LazyLock::new(Mutex::default);
static COMPOSE_PASSES: LazyLock<Mutex<BTreeMap<i32, Arc<Pass>>>> = LazyLock::new(Mutex::default);

fn compile(
    source: &str,
    name: windows::core::PCSTR,
    entry: windows::core::PCSTR,
    target: windows::core::PCSTR,
) -> Result<Vec<u8>> {
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: stable source bytes, static names and valid output pointers.
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            name,
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        if let Some(errors) = errors {
            // SAFETY: compiler-owned blob's byte span.
            let text = unsafe {
                std::slice::from_raw_parts(
                    errors.GetBufferPointer().cast::<u8>(),
                    errors.GetBufferSize(),
                )
            };
            tracing::error!(error=%String::from_utf8_lossy(text), "presentation shader compilation failed");
        }
        return Err(error);
    }
    let code = code.ok_or_else(windows::core::Error::empty)?;
    // SAFETY: compiler-owned blob's byte span, copied before releasing it.
    Ok(unsafe {
        std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize())
    }
    .to_vec())
}

fn make(device: &ID3D12Device, format: DXGI_FORMAT) -> Result<Pass> {
    let (vs, ps) = (
        compile(SOURCE, s!("StarportPresent"), s!("vs"), s!("vs_5_1"))?,
        compile(SOURCE, s!("StarportPresent"), s!("ps"), s!("ps_5_1"))?,
    );
    make_pass(device, format, &vs, &ps)
}

fn make_composite(device: &ID3D12Device, format: DXGI_FORMAT) -> Result<Pass> {
    let (vs, ps) = (
        compile(
            COMPOSE_SOURCE,
            s!("StarportCompose"),
            s!("vs"),
            s!("vs_5_1"),
        )?,
        compile(
            COMPOSE_SOURCE,
            s!("StarportCompose"),
            s!("ps"),
            s!("ps_5_1"),
        )?,
    );
    make_pass(device, format, &vs, &ps)
}

fn make_pass(device: &ID3D12Device, format: DXGI_FORMAT, vs: &[u8], ps: &[u8]) -> Result<Pass> {
    // SAFETY: shaders embed identical signatures; plain object creation.
    unsafe {
        let root: ID3D12RootSignature = device.CreateRootSignature(0, vs)?;
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            VS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: vs.as_ptr().cast(),
                BytecodeLength: vs.len(),
            },
            PS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: ps.as_ptr().cast(),
                BytecodeLength: ps.len(),
            },
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC {
                FillMode: D3D12_FILL_MODE_SOLID,
                CullMode: D3D12_CULL_MODE_NONE,
                DepthClipEnable: true.into(),
                ..Default::default()
            },
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        desc.RTVFormats[0] = format;
        desc.BlendState.RenderTarget[0] = D3D12_RENDER_TARGET_BLEND_DESC {
            SrcBlend: D3D12_BLEND_ONE,
            DestBlend: D3D12_BLEND_ZERO,
            BlendOp: D3D12_BLEND_OP_ADD,
            SrcBlendAlpha: D3D12_BLEND_ONE,
            DestBlendAlpha: D3D12_BLEND_ZERO,
            BlendOpAlpha: D3D12_BLEND_OP_ADD,
            LogicOp: D3D12_LOGIC_OP_NOOP,
            RenderTargetWriteMask: 15,
            ..Default::default()
        };
        Ok(Pass {
            pso: device.CreateGraphicsPipelineState(&desc)?,
            root,
        })
    }
}

/// A legal float-sampled view of a display plane. Typeless resources use the
/// corresponding display type; integer-only formats are deliberately refused.
pub(crate) fn sampled_format(desc: &D3D12_RESOURCE_DESC) -> Option<DXGI_FORMAT> {
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.DepthOrArraySize != 1
        || desc.SampleDesc.Count != 1
        || desc
            .Flags
            .contains(D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)
    {
        return None;
    }
    match desc.Format.0 {
        9 => Some(DXGI_FORMAT_R16G16B16A16_FLOAT),
        23 => Some(DXGI_FORMAT_R10G10B10A2_UNORM),
        27 => Some(DXGI_FORMAT_R8G8B8A8_UNORM),
        90 => Some(DXGI_FORMAT_B8G8R8A8_UNORM),
        10 | 11 | 13 | 24 | 26 | 28 | 29 | 31 | 67 | 87 | 91 => Some(desc.Format),
        _ => None,
    }
}

pub(crate) fn record(
    copy: &HostList,
    device: &ID3D12Device,
    source: &ID3D12Resource,
    output: &ID3D12Resource,
    format: DXGI_FORMAT,
    size: (u32, u32),
) -> Result<()> {
    let pass = {
        let mut passes = PASSES.lock().unwrap_or_else(|e| e.into_inner());
        if let std::collections::btree_map::Entry::Vacant(entry) = passes.entry(format.0) {
            entry.insert(Arc::new(make(device, format)?));
        }
        passes[&format.0].clone()
    };
    // SAFETY: live source resource.
    let d = unsafe { source.GetDesc() };
    if d.SampleDesc.Count != 1
        || d.DepthOrArraySize != 1
        || d.MipLevels != 1
        || d.Flags.0 & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE.0 != 0
        || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
    {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_INVALIDARG,
        ));
    }
    // SAFETY: live non-MSAA color texture and correctly sized descriptor heaps.
    unsafe {
        let srv: ID3D12DescriptorHeap =
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 1,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })?;
        let rtv: ID3D12DescriptorHeap =
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                ..Default::default()
            })?;
        device.CreateShaderResourceView(
            source,
            Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: format,
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_SRV {
                        MipLevels: 1,
                        ..Default::default()
                    },
                },
            }),
            srv.GetCPUDescriptorHandleForHeapStart(),
        );
        let target = rtv.GetCPUDescriptorHandleForHeapStart();
        device.CreateRenderTargetView(output, None, target);
        copy.list.SetPipelineState(&pass.pso);
        copy.list.SetGraphicsRootSignature(&pass.root);
        copy.list.SetDescriptorHeaps(&[Some(srv.clone())]);
        copy.list
            .SetGraphicsRootDescriptorTable(0, srv.GetGPUDescriptorHandleForHeapStart());
        copy.list
            .SetGraphicsRoot32BitConstants(1, 2, [size.0, size.1].as_ptr().cast(), 0);
        copy.list.OMSetRenderTargets(1, Some(&target), false, None);
        copy.list.RSSetViewports(&[D3D12_VIEWPORT {
            Width: size.0 as f32,
            Height: size.1 as f32,
            MaxDepth: 1.0,
            ..Default::default()
        }]);
        copy.list
            .RSSetScissorRects(&[windows::Win32::Foundation::RECT {
                right: size.0 as i32,
                bottom: size.1 as i32,
                ..Default::default()
            }]);
        copy.list
            .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        copy.list.DrawInstanced(3, 1, 0, 0);
        copy.keep(&srv);
        copy.keep(&rtv);
        copy.keep(&pass.pso);
        copy.keep(&pass.root);
        copy.keep(source);
        copy.keep(output);
    }
    Ok(())
}

/// Scales two console display planes to `size` and alpha-composites the
/// premultiplied overlay over the scene. The command list retains every
/// descriptor and resource until its completion fence permits reset.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_composite(
    copy: &HostList,
    device: &ID3D12Device,
    scene: &ID3D12Resource,
    scene_format: DXGI_FORMAT,
    overlay: &ID3D12Resource,
    overlay_format: DXGI_FORMAT,
    output: &ID3D12Resource,
    output_format: DXGI_FORMAT,
    size: (u32, u32),
) -> Result<()> {
    for (resource, format) in [(scene, scene_format), (overlay, overlay_format)] {
        // SAFETY: both resources are live for the recording call.
        let desc = unsafe { resource.GetDesc() };
        if sampled_format(&desc) != Some(format) {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
    }
    let pass = {
        let mut passes = COMPOSE_PASSES.lock().unwrap_or_else(|e| e.into_inner());
        if let std::collections::btree_map::Entry::Vacant(entry) = passes.entry(output_format.0) {
            entry.insert(Arc::new(make_composite(device, output_format)?));
        }
        passes[&output_format.0].clone()
    };
    // SAFETY: live non-MSAA color textures and correctly sized heaps.
    unsafe {
        let srv: ID3D12DescriptorHeap =
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 2,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })?;
        let rtv: ID3D12DescriptorHeap =
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                ..Default::default()
            })?;
        let first = srv.GetCPUDescriptorHandleForHeapStart();
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
            as usize;
        for (index, (resource, format)) in [(scene, scene_format), (overlay, overlay_format)]
            .into_iter()
            .enumerate()
        {
            device.CreateShaderResourceView(
                resource,
                Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: format,
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D12_TEX2D_SRV {
                            MipLevels: 1,
                            ..Default::default()
                        },
                    },
                }),
                D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: first.ptr + index * step,
                },
            );
        }
        let target = rtv.GetCPUDescriptorHandleForHeapStart();
        device.CreateRenderTargetView(output, None, target);
        copy.list.SetPipelineState(&pass.pso);
        copy.list.SetGraphicsRootSignature(&pass.root);
        copy.list.SetDescriptorHeaps(&[Some(srv.clone())]);
        copy.list
            .SetGraphicsRootDescriptorTable(0, srv.GetGPUDescriptorHandleForHeapStart());
        copy.list
            .SetGraphicsRoot32BitConstants(1, 2, [size.0, size.1].as_ptr().cast(), 0);
        copy.list.OMSetRenderTargets(1, Some(&target), false, None);
        copy.list.RSSetViewports(&[D3D12_VIEWPORT {
            Width: size.0 as f32,
            Height: size.1 as f32,
            MaxDepth: 1.0,
            ..Default::default()
        }]);
        copy.list
            .RSSetScissorRects(&[windows::Win32::Foundation::RECT {
                right: size.0 as i32,
                bottom: size.1 as i32,
                ..Default::default()
            }]);
        copy.list
            .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        copy.list.DrawInstanced(3, 1, 0, 0);
        copy.keep(&srv);
        copy.keep(&rtv);
        copy.keep(&pass.pso);
        copy.keep(&pass.root);
        copy.keep(scene);
        copy.keep(overlay);
        copy.keep(output);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_plane_views_are_typed_and_float_sampleable() {
        let desc = |format, flags| D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 1,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: flags,
            ..Default::default()
        };
        assert_eq!(
            sampled_format(&desc(
                DXGI_FORMAT_R8G8B8A8_TYPELESS,
                D3D12_RESOURCE_FLAG_NONE
            )),
            Some(DXGI_FORMAT_R8G8B8A8_UNORM)
        );
        assert_eq!(
            sampled_format(&desc(
                DXGI_FORMAT_B8G8R8A8_TYPELESS,
                D3D12_RESOURCE_FLAG_NONE
            )),
            Some(DXGI_FORMAT_B8G8R8A8_UNORM)
        );
        assert!(
            sampled_format(&desc(DXGI_FORMAT_R8G8B8A8_UINT, D3D12_RESOURCE_FLAG_NONE)).is_none()
        );
        assert!(
            sampled_format(&desc(
                DXGI_FORMAT_R8G8B8A8_UNORM,
                D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE,
            ))
            .is_none()
        );
    }

    #[test]
    fn composite_scales_scene_and_blends_premultiplied_overlay() {
        let h = crate::host::host().expect("D3D12 test device");
        // SAFETY: all resources, descriptors and copy footprints are owned by
        // this test and retained until the synchronous execution completes.
        unsafe {
            let texture = |width: u64| {
                let mut resource = None;
                h.device
                    .CreateCommittedResource(
                        &D3D12_HEAP_PROPERTIES {
                            Type: D3D12_HEAP_TYPE_DEFAULT,
                            ..Default::default()
                        },
                        D3D12_HEAP_FLAG_NONE,
                        &D3D12_RESOURCE_DESC {
                            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                            Width: width,
                            Height: 1,
                            DepthOrArraySize: 1,
                            MipLevels: 1,
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            SampleDesc: DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
                            ..Default::default()
                        },
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        None,
                        &mut resource,
                    )
                    .unwrap();
                resource.unwrap()
            };
            let (scene, overlay, output) = (texture(1), texture(3), texture(3));
            let rtvs: ID3D12DescriptorHeap = h
                .device
                .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                    NumDescriptors: 2,
                    ..Default::default()
                })
                .unwrap();
            let first = rtvs.GetCPUDescriptorHandleForHeapStart();
            let second = D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: first.ptr
                    + h.device
                        .GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV)
                        as usize,
            };
            h.device.CreateRenderTargetView(&scene, None, first);
            h.device.CreateRenderTargetView(&overlay, None, second);
            let list = h.new_list().unwrap();
            list.list
                .ClearRenderTargetView(first, &[0.5, 0.0, 0.0, 1.0], None);
            for (x, color) in [
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.5, 0.0, 0.5],
                [0.0, 0.0, 1.0, 1.0],
            ]
            .iter()
            .enumerate()
            {
                list.list.ClearRenderTargetView(
                    second,
                    color,
                    Some(&[windows::Win32::Foundation::RECT {
                        left: x as i32,
                        right: x as i32 + 1,
                        top: 0,
                        bottom: 1,
                    }]),
                );
            }
            list.list.ResourceBarrier(&[
                transition(
                    &scene,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                ),
                transition(
                    &overlay,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                ),
            ]);
            record_composite(
                &list,
                &h.device,
                &scene,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                &overlay,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                &output,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                (3, 1),
            )
            .unwrap();
            list.list.ResourceBarrier(&[transition(
                &output,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            )]);
            let read = h.read_back_buffer(256).unwrap();
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&output),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&read),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            Width: 3,
                            Height: 1,
                            Depth: 1,
                            RowPitch: 256,
                        },
                    },
                },
            };
            list.list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            list.close();
            h.execute(&[&list]);
            let mut data = std::ptr::null_mut();
            read.Map(0, None, Some(&mut data)).unwrap();
            let pixels: [u8; 12] = std::slice::from_raw_parts(data.cast(), 12)
                .try_into()
                .unwrap();
            read.Unmap(0, None);
            let expected = [128, 0, 0, 255, 64, 128, 0, 255, 0, 0, 255, 255];
            assert!(
                pixels
                    .iter()
                    .zip(expected)
                    .all(|(&actual, expected)| actual.abs_diff(expected) <= 1),
                "unexpected composite {pixels:?}"
            );
        }
    }

    #[test]
    fn downscale_preserves_all_four_quadrants_at_two_scale_factors() {
        let h = crate::host::host().expect("D3D12 test device");
        for side in [4, 8] {
            // SAFETY: all descriptors, clear rectangles and copy footprints
            // below describe the synthetic allocations owned by this test.
            unsafe {
                let make_texture = |width, flags| {
                    let mut resource: Option<ID3D12Resource> = None;
                    h.device
                        .CreateCommittedResource(
                            &D3D12_HEAP_PROPERTIES {
                                Type: D3D12_HEAP_TYPE_DEFAULT,
                                ..Default::default()
                            },
                            D3D12_HEAP_FLAG_NONE,
                            &D3D12_RESOURCE_DESC {
                                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                                Width: width,
                                Height: width as u32,
                                DepthOrArraySize: 1,
                                MipLevels: 1,
                                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                                SampleDesc: DXGI_SAMPLE_DESC {
                                    Count: 1,
                                    Quality: 0,
                                },
                                Flags: flags,
                                ..Default::default()
                            },
                            D3D12_RESOURCE_STATE_RENDER_TARGET,
                            None,
                            &mut resource,
                        )
                        .unwrap();
                    resource.unwrap()
                };
                let input = make_texture(side as u64, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET);
                let output = make_texture(2, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET);
                let rtv: ID3D12DescriptorHeap = h
                    .device
                    .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                        Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                        NumDescriptors: 1,
                        ..Default::default()
                    })
                    .unwrap();
                let cpu = rtv.GetCPUDescriptorHandleForHeapStart();
                h.device.CreateRenderTargetView(&input, None, cpu);
                let list = h.new_list().unwrap();
                let colors = [
                    [1., 0., 0., 1.],
                    [0., 1., 0., 1.],
                    [0., 0., 1., 1.],
                    [1., 1., 1., 1.],
                ];
                for (i, color) in colors.iter().enumerate() {
                    let (x, y) = ((i % 2) as i32 * (side / 2), (i / 2) as i32 * (side / 2));
                    list.list.ClearRenderTargetView(
                        cpu,
                        color,
                        Some(&[windows::Win32::Foundation::RECT {
                            left: x,
                            top: y,
                            right: x + side / 2,
                            bottom: y + side / 2,
                        }]),
                    );
                }
                list.list.ResourceBarrier(&[transition(
                    &input,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                )]);
                record(
                    &list,
                    &h.device,
                    &input,
                    &output,
                    DXGI_FORMAT_R8G8B8A8_UNORM,
                    (2, 2),
                )
                .unwrap();
                list.list.ResourceBarrier(&[transition(
                    &output,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                )]);
                let read = h.read_back_buffer(512).unwrap();
                let from = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&output),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: 0,
                    },
                };
                let to = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&read),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                            Offset: 0,
                            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                                Width: 2,
                                Height: 2,
                                Depth: 1,
                                RowPitch: 256,
                            },
                        },
                    },
                };
                list.list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
                list.close();
                h.execute(&[&list]);
                let mut data = std::ptr::null_mut();
                read.Map(0, None, Some(&mut data)).unwrap();
                let bytes = std::slice::from_raw_parts(data.cast::<u8>(), 512);
                for (i, color) in colors.iter().enumerate() {
                    let at = i / 2 * 256 + i % 2 * 4;
                    assert_eq!(
                        &bytes[at..at + 4],
                        color.map(|v| (v * 255.) as u8),
                        "quadrant {i}, source side {side}"
                    );
                }
                read.Unmap(0, None);
            }
        }
    }
    #[test]
    fn presentation_shaders_and_pipeline_compile_offline() {
        let h = crate::host::host().expect("D3D12 test device");
        for f in [
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_FORMAT_R10G10B10A2_UNORM,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
        ] {
            make(&h.device, f).expect("presentation PSO");
        }
    }
}
