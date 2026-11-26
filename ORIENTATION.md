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

## Implementation added after this baseline

| Area | Current code / boundary |
| --- | --- |
| Resource attribution | `telemetry.rs`: bounded per-frame upload/readback attribution and allocation-offset coverage; sampled every ~2 seconds |
| Resolution ceiling | `display.rs`: monitor/default/explicit ceiling and pure aspect fit; `dxgi.rs`, HLE `lifecycle.rs`, host window/swapchain paths consume it |
| Oversized presentation | `present_blit.rs`: fullscreen linear scaling with owned descriptors; no mismatched CopyResource |
| Multi-mip omission | `resource.rs`: streaming policy / shared retained mip range; `texture.rs`: original source stride, selected host footprints; `descriptor/host.rs`: rebased SRV |
| Missing SRV diagnostics | `fallback.rs`: immutable private-queue initialized typed color resources, bounded cache; UAVs/buffers remain null |
| Descriptor-patched residency | `residency.rs`: invalidation journal, budget sweep, cold candidate order; `descriptor/host.rs`: reverse slot census / patch / requeue; `resource.rs`: epoch-checked reclaim and owned leases |
| Dirty uploads | `watch.rs`: 4 KiB write-watch, 64 KiB section guards; `shadow.rs`: dirty buffer ranges; `texture.rs`: persistent per-mip hashes, compact selected upload footprints |
| Optional GPU upload heaps | `host.rs`: OPTIONS16 query, guarded GPU_UPLOAD selection and ordinary UPLOAD fallback |
| Demand copy-back | Not implemented; `docs/demand-readback.md` explains missing native CPU/alias read barriers. Existing merge/readback remains active |
| Presentation slots | `frames.rs` and `host.rs`: bounded fence-protected slots; guest shader-written buffers / explicit readbacks / callbacks stay synchronous |
| Stalls and memory identity | `watchdog.rs`: bounded event ring / no-present snapshots; loader stack callback is opt-in. `console.rs`: single proportional reported-budget override, independent identity |
| Guarded compute bindings | `binding_trace.rs`, `draw.rs`, `pipeline.rs`: bounded potential producer/consumer overlap from static metadata; shaders remain refused |
| Ambient reach | `ambient_trace.rs`: matched bindings / override hits per present, first output binding and unknown downstream multiply. `ambient.rs` behavior stays unchanged |
| User verification | `VERIFY.md`: exact staged commands, flags/defaults/kill switches, limitations and observable success signals |

Paths without a prefix in this table are under `crates/victory-gpu/src`.
The first table is the recorded pre-change map, not a claim about current defaults.

## Second optimization pass

| Area | Current code / boundary |
| --- | --- |
| Subphases and fixed-scene benchmark | `perf.rs`: bounded per-present phase medians/p95, DXGI/system/process snapshots, F8 30-second JSON; HLE lifecycle and owned host window consume F8 only in bench mode |
| Native allocation accounting | `allocstat.rs`: COM-owned final-release counters; textures/RT/buffer/upload/readback categories, descriptor stride estimate, PSO object count and dimension/BC histogram |
| Real tail residency | `progressive.rs`, `resource.rs`, `texture.rs`, `residency.rs`: sparse real BC tail admission, separate native allocation budget, newest/smaller-first idle promotion with private initialization fence |
| Descriptor transactions | `descriptor/host.rs`: `PromotionPatch`, generation-checked restoration after failed promotion/reclaim; effective mip metadata follows actual backing |
| Eviction eligibility | `residency.rs`: total DXGI pressure, V2 uncompressed/small/recent/churn exclusions, separate hard mapping invalidation journal, per-release logs |
| Dimension target | `resource.rs`: target mip0 dimension with retained 3-mip minimum; explicit fixed-N compatibility and shared SRV/size metadata |
| Self-write filter | `selfwrite.rs`, `shadow.rs`: initialized dirty-page snapshot comparison against merged host bytes; requested reset safely falls back to comparison because guest writers remain concurrent |
| Native protection fixtures | `readback_probe.rs` (tests only), `docs/pass2-readback-boundary.md`: actual Windows fault/write-watch/alias tests; production lazy/adaptive readback still absent |
| Bounded CPU work | `work.rs`, `shadow.rs`, `texture.rs`: dedicated Rayon pool, xxh3-128/BLAKE3 change checks, retained-mip source reads for progressive BC images; no new buffer async or upload copy queue |
| Disposable cache budgets | `memtrim.rs`, `shadow.rs`, `host.rs`: reported memory preset also limits idle snapshots/upload chunks, never decommits live guest bytes or in-flight resources |
| User comparisons | `VERIFY2.md`: ordered F8 comparisons, defaults/kill switches, actual scope and skipped lazy/async stages |

The NVIDIA Wave32/Wave64 guards and ambient shader behavior are unchanged.
The self-write filter preserves the existing write-watch query/reset ordering;
the new retained-mip reader keeps the nonblocking remap read lock.
