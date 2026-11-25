# Resource/pipeline offline validation — 25 November 2026

CLI-only work on Windows/NVIDIA. No game or launcher execution was performed.
The target of <=200 ms world frames / 5+ FPS and readable diffuse surfaces is
not verified. This records what the code and synthetic tests establish.

## Implemented scope and remaining work

| Task | Result / boundary |
| --- | --- |
| 0 | Baseline and current code maps in ORIENTATION.md |
| 1 | Bounded upload/readback attribution, unique/repeat allocation coverage, phase/flight/residency counters |
| 2 | Shared monitor/default/explicit resolution ceiling for reporting, window and presentation; GPU scale for larger sources |
| 3 | N-mip streaming omission, retained tail, SRV/size metadata consistency, immutable staging lifetime |
| 4 | Typed immutable fallback SRVs, exact synthetic texel readbacks; unsupported shapes/UAVs stay null |
| 5 | Idle fence-protected descriptor patch/requeue/reclaim, overwrite hints, owned leases and generation checks |
| 6 | 4 KiB native write-watch, conservative 64 KiB section guards, changed mip/subresource uploads, optional GPU_UPLOAD |
| 7 | Deferred: native CPU/physical-alias read barriers are missing. Existing copy-backs retained, no demand mode advertised |
| 8 | Partial: bounded presentation/no-guest-output flights. Guest CPU outputs, explicit readbacks and callbacks stay synchronous; no separate copy queue |
| 9 | No-present watchdog/history, optional loader stacks, consistent reported pool override with unchanged identity |
| 10 | Four Wave64 programs remain guarded; bounded potential binding edges and per-shader emulation design, no emulation |
| 11 | Per-present matched/hit/first-output diagnostics; downstream albedo multiply explicitly unknown; ambient behavior unchanged |

No actual backlog drain, eviction count during gameplay, shared-memory decrease,
world frame-time decrease, smaller internal targets, Series S boot repair or
visual lighting/RT fix is claimed. Recorded lists can pin cold textures until
reset. Dirty mip selection still hashes/detiles the affected source; it is not
arbitrary tile-level BC updates. Native guest CPU output reads constrain safe
overlap, as explained in [demand-readback.md](demand-readback.md).

## Evidence

External evidence directory:
`A:/starport/runtime/starport/renderer-audit/20261125-resource-pipeline/`.
Logs and owned shader containers remain outside Git.

| Check | Observed result |
| --- | --- |
| fmt --check | Passed |
| workspace check, all targets/features | Passed (`check-final.log`) |
| Affected crates, all features | 492 passed, 0 failed, 3 ignored (`tests.log`) |
| Final GPU suite after fallback lifetime hardening | 157 passed, 0 failed, 1 ignored (`gpu-final.log`), included in the 492 total rather than added again |
| workspace clippy, all targets/features, -D warnings | Passed (`clippy.log`) |
| asset policy | Passed, 194 source files (`policy.log`) |
| Eight reported shader containers | 8/8 translated DXIL validations passed (`dxil.log` / `shaders/`) |
| Separate negative compute PSO diagnostic | Expected failure, exit 101: exactly four Wave64 PSOs refused (`compute-pso-negative.log`); not counted as a passing test |
| Loader diagnostics stack helper | One real test passed with --all-features (`loader-stacks-enabled.log`); an earlier feature-disabled invocation matched zero tests and is not coverage |
| Opt-in ReBAR upload | One smoke test passed; heap_type=5 (GPU_UPLOAD), all 256 readback bytes matched 0xA5 (`rebar.log`) |
| Runner | Python AST parsed successfully; script/launcher not executed |
| diag build | Passed (`build.log`); production binary not executed |

The affected-crate count comprises core 12, GPU 157, HLE 102 + one integration
test, and shader 220. The three ignored tests are two benchmarks and the
inherited strict-join diagnostic. Four compiler-dependent differential wrappers
internally skipped; they remain within Cargo's passing count and do not imply
console compiler execution. Hardware coverage includes the existing 14 LDS
dispatch/readback comparisons. The final GPU run covers descriptor patching,
recorded-list/CPU lease protection, mip-array metadata, movie snapshots, dirty
subresource readback, 4 KiB write-watch, fallback colors and presentation scaling.

The first complete run found one fixture-history failure when Windows reused
a virtual address from an earlier test. The fixture now starts its own journal
history at allocation; all original assertions remain, and the full run passed.

The four repaired pixel shader hashes are `e5b2121a02f4f741`,
`2fd01fca16993001`, `6b6db9f1a526daeb`, `38bf0814e721a1b3`.
The four guarded compute hashes are `2da4beb386509b77`, `369cbe906ea72764`,
`02384a60fda6f2d0`, `6b82bc7d22016cb5`. Offline validation used the eight
previously extracted, hash-matched owned containers, rather than scanning the
entire asset archive again. Valid DXIL does not establish hardware Wave64 PSO
support. No code under `victory-shader`, nor `ambient.rs`, changed after the
separate formatting-only commit `20e5960`.

## Commands actually used

```powershell
$env:CARGO_TARGET_DIR = 'A:/starport/compat/starport_build'
$env:VICTORY_DXIL = 'A:/starport/compat/tools_bin/dxc-1.9.2609/bin/x64/dxil.dll'
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo test -p victory-shader -p victory-gpu -p victory-hle -p victory-core --all-features -- --test-threads=1 --nocapture
cargo test -p victory-gpu --all-features -- --test-threads=1 --nocapture
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask policy

$fixtures = Get-ChildItem 'A:/starport/runtime/starport/renderer-audit/20261125-residency' -Filter '*.cdxil' | Sort-Object Name
$env:STARPORT_SHADER_INPUTS = $fixtures.FullName -join ';'
$env:STARPORT_SHADER_AUDIT_OUT = 'A:/starport/runtime/starport/renderer-audit/20261125-resource-pipeline/shaders'
cargo test -p victory --all-features reported_shaders_validate_offline -- --ignored --nocapture --test-threads=1
# Expected FAIL: four unsupported hardware PSOs; keep separate from suite results.
cargo test -p victory --all-features reported_compute_pipelines_validate_offline -- --ignored --nocapture --test-threads=1
cargo test -p victory-loader --all-features stacks:: -- --test-threads=1 --nocapture
$env:STARPORT_REBAR_UPLOAD = '1'
cargo test -p victory-gpu --all-features host::upload_tests::selected_upload_heap_reaches_a_gpu_copy_and_readback -- --exact --test-threads=1 --nocapture
Remove-Item Env:STARPORT_REBAR_UPLOAD
cargo build --profile diag -p victory --features diagnostics
```

## Build / source integrity

Optimized diagnostics backend:
`A:/starport/compat/starport_build/diag/victory.exe`.
SHA-256: `65dac0ee5f2220ec74563d17e7ab5616ddec3655680467cf4e377e3cc8a75c78`.
Code at build: `b3cb002` plus pending documentation/launcher-comment changes.
The final documentation commit does not alter executable source behavior.

Original title executable SHA-256 remains
`e95020b2fa4a721db3df59fdecf75621730ac559d8835252f6c3b658d5a08993`.
This verifies the executable; it is not a new complete 224 GB asset hash audit.
The outer `A:/starport/Play-Starport.cmd` has matching launcher comments/mip
defaults and is outside the Git repository. Source commits remain local.

Behavioral code/launcher files changed relative to the separate formatting
commit (the corresponding docs and README also changed):

```text
Play-Starport.cmd
apps/victory/src/run.rs
crates/victory-gpu/src/ambient_trace.rs
crates/victory-gpu/src/binding_trace.rs
crates/victory-gpu/src/console.rs
crates/victory-gpu/src/descriptor.rs
crates/victory-gpu/src/descriptor/host.rs
crates/victory-gpu/src/display.rs
crates/victory-gpu/src/draw.rs
crates/victory-gpu/src/dxgi.rs
crates/victory-gpu/src/fallback.rs
crates/victory-gpu/src/fence.rs
crates/victory-gpu/src/frames.rs
crates/victory-gpu/src/host.rs
crates/victory-gpu/src/lib.rs
crates/victory-gpu/src/list.rs
crates/victory-gpu/src/pipeline.rs
crates/victory-gpu/src/present_blit.rs
crates/victory-gpu/src/queue.rs
crates/victory-gpu/src/residency.rs
crates/victory-gpu/src/resource.rs
crates/victory-gpu/src/shadow.rs
crates/victory-gpu/src/telemetry.rs
crates/victory-gpu/src/texture.rs
crates/victory-gpu/src/watch.rs
crates/victory-gpu/src/watchdog.rs
crates/victory-hle/src/lib.rs
crates/victory-hle/src/modules/lifecycle.rs
scripts/starport.py
```

Use [VERIFY.md](../VERIFY.md) for exact ordered user-run comparisons and kill
switches; [ORIENTATION.md](../ORIENTATION.md) maps the files, and
[wave64-emulation.md](wave64-emulation.md) records the per-shader design.
