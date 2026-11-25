# Renderer orientation

Source baseline: `38d1ca94e5c8c17f7d6a6543241bb1a876fe9043`.
This map was checked against the code before changing behavior. No game launch.

| Area | Location and entry points |
| --- | --- |
| Display mode reported/enumerated | `crates/victory-gpu/src/dxgi.rs`: `display_size`, `modes`; currently defaults to 3840x2160, with `VICTORY_DISPLAY_MODE` override |
| Guest window | `crates/victory-hle/src/modules/lifecycle.rs`: `CreateWindowExA/W`, `fit`, `fit_into`; fits top-level windows to the primary work area |
| Host window/present size | `crates/victory-gpu/src/host.rs`: `Host::present`, `Host::presenter`, `window`, `make_window`; currently copies a same-size source into swapchain buffers |
| Texture upload and admission backlog | `crates/victory-gpu/src/texture.rs`: `Staging::refresh`, `take_upload`, `Upload::record`; `draw.rs`: `heap_textures`; `descriptor/host.rs`: waiting view resolution; `resource.rs`: `room`, `hosted` |
| Top-mip omission and SRV rebase | `resource.rs`: `streaming_first_mip`, `host_first_mip`, `texture_size_words`; `descriptor/host.rs`: SRV creation; `texture.rs`: staging layout/upload footprints |
| Residency | `residency.rs`: `track`, `descriptor_kept`, `prepare`, `sweep`, `forget`; conservative bindless-heap protection; `resource.rs`: `HostTexture`, `let_go_idle` |
| Buffer uploads | `host.rs`: `HostList::defer_copy`, `defer_refresh`, `Batch`; `memory.rs`, `shadow.rs` own buffer shadow state; `watch.rs` journals Windows write-watch and remaps |
| Copy-back | `host.rs`: `read_backs`, `hand_back`, `Host::card_back`, relay landing; `shadow.rs`: `copy_back*`; `HostList::defer_read_back` retains explicit texture-to-memory reads |
| Frame phases | `host.rs`: `Host::submit`, settle/flight landing, `Meter::frame`; `PREPARING` and submission phase counters. Existing GPU-wait metric is CPU fence-wait wall time, not GPU timestamps |
| Memory budgets | `host.rs`: `Host::video_memory`; `copystat.rs`: `log_memory`; `resource.rs`: `room`, `KINDS`, `HOST_TEXTURES` |
| Series S profile | `console.rs`: `Console::title_memory`, profile selection; `apps/victory/src/run.rs`; HLE `xmem.rs`: `title_budget`, `XMemGetWorkingSetStatistics`; `kernel.rs`: `GlobalMemoryStatusEx`; DXGI adapter reporting |
| Refused compute dispatches | `pipeline/host.rs`: host compute PSO creation/refusal; `draw.rs`: `State::dispatch` and indirect paths. Wave64 completeness guards must remain |
| Ambient fallback | `ambient.rs`: `raise`; `draw.rs`: `State::ambient` recognizes the screen-probe resolve. This writes a constant indirect term; the downstream material multiplication is not inferred from that hook |
| Existing overlap model | `host.rs`: `Flight`, `Line`, `Sent`, `late_blocked`, `blocked_by`, relay worker; already has bounded asynchronous submission/landing experiments. Shared snapshots and CPU read hazards constrain extending it |
| Launch/environment recording | `scripts/starport.py`, `Play-Starport.cmd`; runner filters inherited `VICTORY_`/`STARPORT_` variables and rebuilds the child environment, so new switches need explicit forwarding |

Game files, shader fixtures, logs, build outputs and runtime captures stay outside
the source tree. Existing remap locks, NVIDIA Wave32 handling, pixel shader
regressions and RT completeness guards are required invariants.
