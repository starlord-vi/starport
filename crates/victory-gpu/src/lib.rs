//! D3D12.X on host D3D12.
//!
//! D3D12.X records commands from code inlined into the title, which writes
//! packets into the command list while its put pointer is below a limit and
//! calls the list's method otherwise. Our lists keep the limits at zero, so
//! every command arrives here as a call, to be run on host D3D12. The
//! title's GPU addresses are CPU addresses (console memory is unified).
//!
//! [`device`] is the device object titles create, [`dxgi`] its adapter side,
//! [`resource`] heaps and resources, [`fence`] fences, [`queue`] command
//! queues, [`list`] command allocators and lists, [`descriptor`]
//! descriptor heaps, [`root`] root signatures, [`query`] query heaps,
//! [`signature`] command signatures, [`pipeline`] pipeline states,
//! [`frame`] frame pacing, [`object`] what all console graphics objects
//! share, [`console`] which console model the title sees. Planned:
//! `present` (swap chain).

mod allocstat;
#[cfg(windows)]
mod ambient;
mod ambient_trace;
mod perf;
pub use perf::hotkey as bench_hotkey;
#[cfg(windows)]
mod binding_trace;
#[cfg(all(windows, feature = "diagnostics"))]
mod capture;
pub mod console;
#[cfg(all(windows, feature = "diagnostics"))]
mod copystat;
pub mod descriptor;
pub mod device;
pub mod display;
#[cfg(windows)]
mod draw;
pub mod dxgi;
#[cfg(windows)]
mod fallback;
pub mod fence;
pub mod frame;
#[cfg(windows)]
mod frames;
#[cfg(windows)]
mod host;
#[cfg(windows)]
mod indirect;
#[cfg(windows)]
mod inplace;
pub mod list;
#[cfg(windows)]
mod memory;
pub mod object;
pub mod pipeline;
#[cfg(windows)]
mod present_blit;
#[cfg(windows)]
mod primitive;
pub mod query;
pub mod queue;
#[cfg(any(windows, test))]
mod readback;
#[cfg(windows)]
mod residency;
pub mod resource;
pub mod root;
#[cfg(windows)]
mod sampler;
#[cfg(windows)]
pub mod shader_cache;
#[cfg(windows)]
mod shadow;
pub mod signature;
#[cfg(windows)]
mod telemetry;
#[cfg(windows)]
mod texture;
#[cfg(windows)]
mod watch;
#[cfg(windows)]
mod watchdog;
#[cfg(windows)]
pub use watchdog::{on_stacks as on_watchdog_stacks, start as start_watchdog};

#[cfg(windows)]
pub use host::on_failure;
#[cfg(windows)]
pub use inplace::{title_memory_freeing, title_memory_section, title_memory_view};
#[cfg(windows)]
pub use watch::{
    title_memory_reading, title_memory_remapped, title_memory_remapping,
    writing as kernel_will_write,
};
