//! Opt-in, immutable diagnostic SRVs. UAV writes keep their typed null views.
//! Initialization uses a private queue: descriptor publication can run while
//! the normal submission mutex is held, so Host::execute would deadlock here.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex, OnceLock};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};
use windows::core::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    Black,
    Grey,
    Magenta,
    White,
}
fn parse(value: &str) -> Option<Color> {
    match value {
        "black" => Some(Color::Black),
        "grey" | "gray" => Some(Color::Grey),
        "magenta" => Some(Color::Magenta),
        "white" => Some(Color::White),
        _ => None,
    }
}
fn rgba(color: Color) -> [f32; 4] {
    match color {
        Color::Black => [0., 0., 0., 1.],
        Color::Grey => [128. / 255., 128. / 255., 128. / 255., 1.],
        Color::Magenta => [1., 0., 1., 1.],
        Color::White => [1.; 4],
    }
}
fn selected() -> Color {
    static COLOR: OnceLock<Color> = OnceLock::new();
    *COLOR.get_or_init(|| {
        let asked=std::env::var("STARPORT_FALLBACK_COLOR").unwrap_or_else(|_|"black".into());
        let color=parse(&asked).unwrap_or_else(|| {
            tracing::warn!(value=asked,"invalid fallback color; retaining black null views");Color::Black
        });
        tracing::info!(target:"starport::fallback",?color,"missing texture SRV diagnostic; black restores original null behavior; UAVs remain null");
        color
    })
}
static WRITES: AtomicU64 = AtomicU64::new(0);
static DISABLED: AtomicBool = AtomicBool::new(false);
pub(crate) fn writes() -> u64 {
    WRITES.load(Relaxed)
}

// COM objects are immutable/free-threaded; resources are held for the process.
struct Image(ID3D12Resource);
unsafe impl Send for Image {}
unsafe impl Sync for Image {}
type Key = (u32, u32, u16);
static IMAGES: LazyLock<Mutex<BTreeMap<Key, Image>>> = LazyLock::new(Mutex::default);

fn numeric_format(original: u32) -> DXGI_FORMAT {
    match original {
        3 | 7 | 12 | 17 | 25 | 30 | 36 | 42 | 50 | 57 | 62 => DXGI_FORMAT_R32G32B32A32_UINT,
        4 | 8 | 14 | 18 | 32 | 38 | 43 | 52 | 59 | 64 => DXGI_FORMAT_R32G32B32A32_SINT,
        _ => DXGI_FORMAT_R8G8B8A8_UNORM,
    }
}

fn shape(view: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> Option<(D3D12_RESOURCE_DIMENSION, u16, u32)> {
    // SAFETY: the view dimension selects its corresponding union member.
    let (dimension, layers, samples) = unsafe {
        match view.ViewDimension {
            D3D12_SRV_DIMENSION_TEXTURE1D => (D3D12_RESOURCE_DIMENSION_TEXTURE1D, 1, 1),
            D3D12_SRV_DIMENSION_TEXTURE1DARRAY => (
                D3D12_RESOURCE_DIMENSION_TEXTURE1D,
                view.Anonymous.Texture1DArray.ArraySize,
                1,
            ),
            D3D12_SRV_DIMENSION_TEXTURE2D => (D3D12_RESOURCE_DIMENSION_TEXTURE2D, 1, 1),
            D3D12_SRV_DIMENSION_TEXTURE2DARRAY => (
                D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                view.Anonymous.Texture2DArray.ArraySize,
                1,
            ),
            D3D12_SRV_DIMENSION_TEXTURE2DMS => (D3D12_RESOURCE_DIMENSION_TEXTURE2D, 1, 2),
            D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY => (
                D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                view.Anonymous.Texture2DMSArray.ArraySize,
                2,
            ),
            D3D12_SRV_DIMENSION_TEXTURE3D => (D3D12_RESOURCE_DIMENSION_TEXTURE3D, 1, 1),
            D3D12_SRV_DIMENSION_TEXTURECUBE => (D3D12_RESOURCE_DIMENSION_TEXTURE2D, 6, 1),
            D3D12_SRV_DIMENSION_TEXTURECUBEARRAY => (
                D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                view.Anonymous.TextureCubeArray.NumCubes.checked_mul(6)?,
                1,
            ),
            _ => return None,
        }
    };
    if layers == 0 || layers > 2048 {
        return None;
    }
    Some((dimension, layers as u16, samples))
}

fn make(
    device: &ID3D12Device,
    dimension: D3D12_RESOURCE_DIMENSION,
    layers: u16,
    samples: u32,
    format: DXGI_FORMAT,
    color: Color,
) -> Result<ID3D12Resource> {
    // SAFETY: finite tiny color texture; private initialization queue touches
    // only this texture. Every object remains alive through its fence.
    unsafe {
        let mut image: Option<ID3D12Resource> = None;
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: dimension,
                Width: 1,
                Height: 1,
                DepthOrArraySize: layers,
                MipLevels: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: samples,
                    Quality: 0,
                },
                Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            None,
            &mut image,
        )?;
        let image = image.ok_or_else(windows::core::Error::empty)?;
        let rtv: ID3D12DescriptorHeap =
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                NumDescriptors: 1,
                ..Default::default()
            })?;
        let handle = rtv.GetCPUDescriptorHandleForHeapStart();
        device.CreateRenderTargetView(&image, None, handle);
        let queue: ID3D12CommandQueue =
            device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default())?;
        let allocator: ID3D12CommandAllocator =
            device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
        let list: ID3D12GraphicsCommandList =
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
        list.ClearRenderTargetView(handle, &rgba(color), None);
        list.ResourceBarrier(&[crate::host::transition(
            &image,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
                | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        )]);
        list.Close()?;
        let submitted: ID3D12CommandList = windows::core::Interface::cast(&list)?;
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
        // Register completion before execution, avoiding a resource lifetime
        // gap if registration fails. After submission an uncertain completion
        // must retain the event as well as the command's GPU objects.
        let event = CreateEventW(None, false, false, None)?;
        if let Err(error) = fence.SetEventOnCompletion(1, event) {
            let _ = CloseHandle(event);
            return Err(error);
        }
        queue.ExecuteCommandLists(&[Some(submitted)]);
        let wait = (|| -> Result<()> {
            queue.Signal(&fence, 1)?;
            if WaitForSingleObject(event, INFINITE) != WAIT_OBJECT_0
                || fence.GetCompletedValue() != 1
            {
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_FAIL,
                ));
            }
            Ok(())
        })();
        if let Err(error) = wait {
            // Submission already happened, but its completion is unknown.
            // Keep everything alive rather than release GPU-referenced memory
            // or an event the fence may still signal. Disable future attempts
            // so this exceptional quarantine cannot grow without bound.
            DISABLED.store(true, Relaxed);
            std::mem::forget((image, rtv, queue, allocator, list, fence));
            tracing::error!(%error, "fallback initialization completion unknown; retained submission objects and disabled color diagnostics");
            return Err(error);
        }
        let _ = CloseHandle(event);
        Ok(image)
    }
}

/// Returns false for black, buffers/UAVs, unknown shapes or invalid arrays.
/// MSAA's exact sample count is not in an SRV: keep it null rather than guess.
pub(crate) fn write(
    mut view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    at: D3D12_CPU_DESCRIPTOR_HANDLE,
) -> bool {
    let color = selected();
    if color == Color::Black || DISABLED.load(Relaxed) {
        return false;
    }
    let Some((dimension, layers, samples)) = shape(&view) else {
        return false;
    };
    if samples != 1 {
        static MSAA: std::sync::Once = std::sync::Once::new();
        MSAA.call_once(|| {
            tracing::warn!(
                "fallback color unavailable for unknown MSAA sample count; typed null retained"
            )
        });
        return false;
    }
    let format = numeric_format(view.Format.0 as u32);
    let key = (dimension.0 as u32, format.0 as u32, layers);
    let Some(host) = crate::host::host() else {
        return false;
    };
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    if DISABLED.load(Relaxed) {
        return false;
    }
    if images.len() >= 64 && !images.contains_key(&key) {
        static LIMIT: std::sync::Once = std::sync::Once::new();
        LIMIT.call_once(|| {
            tracing::warn!("diagnostic fallback variant limit reached; retaining typed null")
        });
        return false;
    }
    if let std::collections::btree_map::Entry::Vacant(entry) = images.entry(key) {
        match make(&host.device, dimension, layers, samples, format, color) {
            Ok(image) => {
                entry.insert(Image(image));
            }
            Err(error) => {
                tracing::error!(%error,?key,"diagnostic fallback creation failed; retaining typed null");
                return false;
            }
        }
    }
    view.Format = format;
    view.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    // SAFETY: matching active union and a tiny resource of that dimension.
    unsafe {
        match view.ViewDimension {
            D3D12_SRV_DIMENSION_TEXTURE1D => {
                view.Anonymous.Texture1D = D3D12_TEX1D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                }
            }
            D3D12_SRV_DIMENSION_TEXTURE1DARRAY => {
                view.Anonymous.Texture1DArray = D3D12_TEX1D_ARRAY_SRV {
                    MipLevels: 1,
                    ArraySize: layers.into(),
                    ..Default::default()
                }
            }
            D3D12_SRV_DIMENSION_TEXTURE2D => {
                view.Anonymous.Texture2D = D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                }
            }
            D3D12_SRV_DIMENSION_TEXTURE2DARRAY => {
                view.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_SRV {
                    MipLevels: 1,
                    ArraySize: layers.into(),
                    ..Default::default()
                }
            }
            D3D12_SRV_DIMENSION_TEXTURE3D => {
                view.Anonymous.Texture3D = D3D12_TEX3D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                }
            }
            D3D12_SRV_DIMENSION_TEXTURECUBE => {
                view.Anonymous.TextureCube = D3D12_TEXCUBE_SRV {
                    MipLevels: 1,
                    ..Default::default()
                }
            }
            D3D12_SRV_DIMENSION_TEXTURECUBEARRAY => {
                view.Anonymous.TextureCubeArray = D3D12_TEXCUBE_ARRAY_SRV {
                    MipLevels: 1,
                    NumCubes: u32::from(layers) / 6,
                    ..Default::default()
                }
            }
            _ => return false,
        }
        host.device
            .CreateShaderResourceView(&images[&key].0, Some(&view), at);
    }
    WRITES.fetch_add(1, Relaxed);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialized_debug_colors_have_the_expected_gpu_texels() {
        let h = crate::host::host().unwrap();
        for (color, expected) in [
            (Color::Grey, [128, 128, 128, 255]),
            (Color::Magenta, [255, 0, 255, 255]),
            (Color::White, [255; 4]),
        ] {
            let image = make(
                &h.device,
                D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                1,
                1,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                color,
            )
            .unwrap();
            let list = h.new_list().unwrap();
            let read = h.read_back_buffer(256).unwrap();
            // SAFETY: synthetic 1x1 image and a 256-byte pitched readback.
            unsafe {
                list.list.ResourceBarrier(&[crate::host::transition(
                    &image,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
                        | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                )]);
                let from = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::transmute_copy(&image),
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
                                Width: 1,
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
                let mut bytes = std::ptr::null_mut();
                read.Map(0, None, Some(&mut bytes)).unwrap();
                assert_eq!(std::slice::from_raw_parts(bytes.cast::<u8>(), 4), expected);
                read.Unmap(0, None);
            }
        }
    }
    #[test]
    fn colors_and_shapes_reject_unknowns() {
        assert_eq!(parse("grey"), Some(Color::Grey));
        assert_eq!(parse("invalid"), None);
        assert_eq!(rgba(Color::Magenta), [1., 0., 1., 1.]);
        assert!(shape(&D3D12_SHADER_RESOURCE_VIEW_DESC::default()).is_none());
    }
    #[test]
    fn immutable_fallback_initializes_offline_for_each_texture_shape() {
        let h = crate::host::host().unwrap();
        for (dim, layers) in [
            (D3D12_RESOURCE_DIMENSION_TEXTURE1D, 1),
            (D3D12_RESOURCE_DIMENSION_TEXTURE1D, 8),
            (D3D12_RESOURCE_DIMENSION_TEXTURE2D, 1),
            (D3D12_RESOURCE_DIMENSION_TEXTURE2D, 12),
            (D3D12_RESOURCE_DIMENSION_TEXTURE3D, 1),
        ] {
            for f in [
                DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_FORMAT_R32G32B32A32_UINT,
                DXGI_FORMAT_R32G32B32A32_SINT,
            ] {
                make(&h.device, dim, layers, 1, f, Color::Grey).expect("fallback initialization");
            }
        }
    }
}
