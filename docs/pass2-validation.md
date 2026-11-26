# Second renderer pass — 26 November 2026

CLI-only Windows/NVIDIA work. Neither the game nor either launcher was run.
The previous pass ended at `e300835`; behavioral code for this pass is at
`9eb4c0d`. All commits remain local. Use [VERIFY2.md](../VERIFY2.md) for the
ordered manual comparisons and [ORIENTATION.md](../ORIENTATION.md) for code.

## Implemented scope

| Task | Result and boundary |
| --- | --- |
| 0 | Per-present median/p95 subphases, native COM-lifetime allocation categories, DXGI local/non-local and CPU/process memory, BC/dimension histogram, F8 30-second benchmark JSON |
| 1 | Actual <=64px BC mip tails where source pages exist, strict separate allocation budget, newest-use/smaller-target promotion at idle, initialized backing and transactional descriptor restoration |
| 2 | Total DXGI pressure policy with uncompressed/small/recent/churn exclusions, persistent re-upload count, separate hard mapping invalidations, per-release telemetry; no evicted CPU pixel cache |
| 3 | Dimension-based mip target with retained three-mip minimum, explicit legacy override compatibility and actual-generation SRV/size metadata |
| 4a | Native write-watch dirty-page comparison against merged host bytes. Requested reset falls back to comparison; no unsafe post-merge reset erases concurrent guest writes |
| 4b–d | Native protection/alias conformance fixtures pass; production lazy readback and 60-frame adaptive policy are **not implemented** |
| 5 | Bounded Rayon CPU scans/copies/hashes, xxh3-128 with full BLAKE3 kill switch, retained-mip reads for progressive BC. Existing batched write-watch retained. **No new copy queue or lazy buffer async overlap** |
| 6 | One-time optional exposure warning and budget-scaled strict idle snapshot/upload cache caps. **No new guest commit cap/demand-commit implementation** |

Native private-memory guard/NOACCESS and write-watch composition is verified.
That does not establish safe publication into private guest memory. Mapped
host-only aliases support publication but lack native write-watch, and all
aliases, kernel I/O, mapping lifetimes and fault-worker locks still need
coverage. See [the production readback boundary](pass2-readback-boundary.md).
Buffer outputs therefore retain eager CPU visibility and synchronous submission.

## Offline results

Evidence remains outside Git under `A:/starport/runtime/starport/`.

| Check | Result / evidence |
| --- | --- |
| Formatting | `cargo fmt --all -- --check`, passed (`pass2-final-fmt.log`) |
| Workspace check | All targets/features, passed (`pass2-final-check.log`) |
| Affected suites | 513 passed, 0 failed, 3 ignored; full affected run plus the final GPU re-run replacing its earlier count (`pass2-final-tests.log`, `pass2-final-gpu.log`) |
| Workspace clippy | All targets/features, `-D warnings`, passed (`pass2-final-clippy.log`) |
| Asset policy | Passed, 204 source files (`pass2-final-policy.log`) |
| Shader translation/DXIL | All eight previously reported owned containers pass (`pass2-final-dxil.log`) |
| Separate compute PSO negative diagnostic | Expected failure, exit 101: the four guarded Wave64 pipelines remain unsupported (`pass2-final-compute-negative.log`); not a passing test |
| Benchmark export | Actual file overwrite, no leftover temporary file, source-tree rejection; Python JSON parser checks all 11 numeric phase summaries and null unavailable memory fields in a synthetic report |
| Runner | Python AST parsed; all pass-2 switches explicitly forwarded. The runner was not executed |
| Diagnostics build | Optimized `diag` build passed (`pass2-final-build.log`); production executable not run |

The 513 tests comprise core 12, GPU 178, HLE 102 plus one integration test,
and shader 220. Three ignored tests are two benchmarks and the inherited
strict-join diagnostic. Four compiler-dependent differential wrappers internally
skip without a console compiler; their passing Cargo wrappers are not console
execution coverage. The intentional worker-panic fixture prints a panic and
passes by confirming propagation and pool reuse.

New native coverage includes actual partial-source tail upload and initialized
promotion with raw BC GPU readback, refused native leases with descriptor
rollback, persistent re-upload count, real self-merge suppression followed by
a detected guest edit, a guest write after snapshot capture remaining dirty,
and native protection/write-watch/alias behavior. The alias fixture's GPU
completion/data is explicitly mocked; it is not a production demand pager.

The synthetic benchmark report is
`renderer-audit/20261126-pass2/synthetic-bench.json`, with two artificial 25/75us
samples. It establishes export/quantile/JSON behavior, not F8 delivery or actual
scene performance. DXIL outputs remain in that audit directory's `shaders/`.

Optimized backend: `A:/starport/compat/starport_build/diag/victory.exe`.
SHA-256: `26023fcff974ca06f535b5f3488b713a0c4da79a07e3d93f622f0db782c94a16`.
Built from `9eb4c0d`; the following report commit changes documentation only.
Original title executable SHA-256 remains
`e95020b2fa4a721db3df59fdecf75621730ac559d8835252f6c3b658d5a08993`.
This checks the original executable, not a fresh complete asset-archive audit.
The outer `A:/starport/Play-Starport.cmd` received matching guide comments only.

## Runtime claims and risks

- No world FPS, <=400/200ms target, backlog reduction, copy-back improvement,
  shared-memory decrease or visual recovery is established by these checks.
  The user must repeat the same scene with F8 and compare workload/draw counts.
- A tail needs actual readable source pixels. Completely unmapped/missing data,
  unsupported layouts, non-BC resources and GPU-written targets retain the
  conservative path. No placeholder is called recovered texture content.
- Native allocation alignment matters: a tiny committed tail can cost 64KiB.
  A 512MiB cap fits at most 8,192 such minimum allocations, fewer with larger
  images; slots can share backing, but not every large backlog can fit at once.
  A later packed small-resource allocator could reduce this overhead.
- Promotion currently initializes at most eight attempts and ~16ms of work per
  frame, with indivisible initialization allowed to exceed that soft limit.
  Recorded/dynamic/CPU leases and missing source postpone it. Uncertain private
  queue completion quarantines its resources and stops further initialization.
- V2 churn exclusions intentionally pin some images. They can reduce immediate
  reclaimable bytes; use total DXGI and due/partial counters to judge admission.
- Private write-watch clean ranges skip texture reads; alias-backed retained
  pixels are still checked conservatively. Dirty tiled mips are hashed whole,
  not claimed as general 4KiB-only texture hashing. Sparse page/block traversal
  and worker dispatch can cost CPU time; independent kill switches support A/B.
- Timers sum inclusive operation durations over workers and can overlap. They
  are not disjoint preparing slices or GPU timestamp measurements. Descriptor
  bytes are stride estimates, driver PSO bytes are unavailable/null.
- Cache caps free only disposable host allocations. Lower caps can increase
  allocation churn. Live guest pages, dirty data and in-flight buffers are
  retained. Reported title budgets remain consistent but do not forcibly
  decommit mapped guest memory; RAM below 85% remains a measurement target.
- NVIDIA wave handling, guarded compute shaders and ambient shader behavior
  are preserved. Lighting/RT/boat correctness is not solved by residency work.

The next architecture requirement for lazy buffer overlap is a versioned
managed guest allocation/alias registry, software write journaling where needed,
independent fault publication workers and CPU/kernel read barriers. All those
contracts must pass tests before removing synchronous output waits. If the
new benchmark instead shows command translation dominating, prioritize measured
generation-validated immutable translation reuse first.

## Local commits

`cee2f6d` instrumentation/benchmark; `2cc2d0e` real tails/promotion;
`87f5dc5` eviction safeguards; `7ad973d` dimension target;
`b2c0d76` self-write comparison; `2b6e904` protection fixtures/boundary;
`85a3460` bounded CPU/hash/retained reads; `43101f4` cache budgets/warning;
`9eb4c0d` benchmark export and runner/documentation checks.

Rayon 1.12.0 / rayon-core 1.13.0 (MIT OR Apache-2.0) and xxhash-rust 0.8.21
(BSL-1.0) are used through Cargo dependencies with exact resolutions/checksums
in Cargo.lock. Their licenses were verified from downloaded registry manifests.
No upstream renderer implementation was copied for this pass.
