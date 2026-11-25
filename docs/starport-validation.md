# Local validation: 25 November 2026

This records earlier visual runs and the earlier workspace suite. The later
CLI-only resource/pipeline work has separate evidence in
[renderer verification](../VERIFY.md); no new gameplay measurement is implied.

Windows 11, Intel i5-13400F, NVIDIA RTX 5060 (8 GB), 32 GB RAM. Original
executable SHA-256:
`e95020b2fa4a721db3df59fdecf75621730ac559d8835252f6c3b658d5a08993`.

## Reproduced visually

- Series X account screen, animated intro, navigation/text/subtitle/brightness
  setup, illustrated backgrounds and opening-credit graphics render.
- Keyboard Right changed Minimap to Minimap & Compass, Left restored it,
  Down moved between menu entries and Enter advanced the screens.
- F6 reached the capture hook. Mouse motion/trigger conversion has unit
  coverage; physical mouse feel and every gameplay binding remain unverified.
- Series S produced a black first-boot screen. Its additional presentation
  plane/upscaler support is not claimed fixed.

The successful 600-second diagnostic uses executable SHA-256
`299a036f7f1b7244625ea040e760498b6349c180d801b6be7cd3bdb3c638245c`,
with relay mode 2 and the BVH-builder request enabled. It exited 0 after title
suspension; original-file size/mtime and original binary SHA checks passed.
Its logs, captures and source snapshot are outside Git:
`../../runtime/starport/runs/20261125T114450.029236Z`.

## Tests actually run

```powershell
$env:CARGO_TARGET_DIR = 'A:\starport\compat\starport_build'
$env:VICTORY_DXIL = 'A:\starport\compat\tools_bin\dxc-1.9.2609\bin\x64\dxil.dll'
cargo test --workspace --all-features --no-fail-fast -- --test-threads=1 --nocapture
cargo xtask policy
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Result: **518 passed, 0 failed, 7 ignored** across 23 result targets, including
empty doc-test targets. Four internal differential tests skipped because no
console compiler was supplied; passing wrappers do not count as console
execution coverage. Clippy completed without warnings. The seven ignored tests are three benchmarks, three
external reference-data tests and the inherited strict-join diagnostic.

The LDS flag lowering additionally ran 14 hardware dispatch/readback comparisons
over both forms and lane counts spanning the two host waves. Readback coalescing
tests check exact bytes, gaps, resource changes, overflow and real WARP copies.
No assertions were weakened to obtain these results.

Full test log: `../../compat/logs/starport-full-tests.log`. The package-write
test verifies rejected mutation attempts and preserved real reads. Python source
was added to the policy's source-type allowlist; binary/encoded-data checks remain.

## Performance limits observed

Menus ran around 26-31 presented frames/sec with no pending pipelines. This is
application presentation cadence, not unique rendered frames or display scanout.
Capture was enabled and these figures are diagnostic observations.

The cold world-loading/credit sequence was incomplete and slow: approximately
0.3-1.2 presented frames/sec, about 900-1,000 unfinished pipelines and dropped
passes. In one sample a frame took 3,914 ms: texture preparation accounted for
3,310 ms, CPU fence waits 53 ms, and copy-back 160 ms. The inherited log labels
the fence-wait counter "on the GPU"; it is not a GPU execution timestamp.
Phase totals can overlap; they
must not be added into a new frame-time estimate.

The RT builder remains disabled by its completeness guard despite being requested.
The run also recorded three compute pipelines refused by D3D12 and three
divergent first-lane read shaders failing translation. One fixed LDS flag path is not a
complete RT solution. Lighting, boat height, full world rendering and 60 FPS
have not been verified or declared fixed.

The host pipeline cache saved 817 newly made pipelines and 75 loaded pipelines
in the final log. A warm run and an equivalent baseline are still needed before
claiming a gameplay speed-up. The readback batching improvement is validated for
correctness, not asserted to solve the overall frame-time problem.

## 2026-11-25: Series S, residency and shader follow-up

See [starport-residency.md](starport-residency.md) for the exact changed files,
new runtime switches, provenance and A/B commands. CLI-only checks report
470 passing affected-crate tests, 3 ignored benchmarks, and 4 internal
compiler-dependent skips within the passing count. Workspace check, Clippy
with warnings denied, and the 180-file asset policy pass. All eight reported
shader containers pass DXIL validation offline. The four previously failing
pixel shaders now translate; the four compute PSOs remain refused on NVIDIA
because they still require Wave64. Those failures are recorded separately.

The local optimized diagnostic backend was rebuilt, but neither it nor the
launcher was run. World FPS, selected resolution and visual correctness have
not been measured for these changes. `ambient.rs` matches the user's last-run
source snapshot byte-for-byte; the remap-lock and Wave64 regression-test patches
also match that snapshot. Existing NVIDIA Wave32 lowering remains in place.

## Subsequent user-run logs: 2026-11-25

The user ran the rebuilt backend; analysis of these logs did not launch the
game or apply further renderer changes. Backend SHA-256:
`393d00009c9f7f31e486bb13151e8e2ebe1cead04744e4a8b1da48184c09efa0`.

- `20261125T163304.019122Z`: Series X, top-mip clamp enabled. Final 30 seconds:
  1.263 FPS, median 714.46 ms, p95 1,058.51 ms. Four repaired pixel shaders
  created host mesh pipelines; the four Wave64 compute PSOs and a separate
  divergent lane-mask compute translation still fail. Ambient fallback was
  active at 2560x1440. The final memory sample had 5,220 MiB local GPU memory,
  5,031 MiB shared GPU memory, 3,487 MiB system RAM free, zero new residency
  evictions and 13,862 waiting texture descriptor slots. CPU texture staging
  was down to zero MiB and remap-lock wait remained zero ms. The active working
  set and buffer snapshot/copy-back work remain substantial bottlenecks.
- `20261125T162734.033539Z`: Series S, full mip chain. Final 30 seconds:
  30.10 FPS, median 33.34 ms, but only roughly 11 draws/frame, one dispatch
  and eight host textures. This was a menu/loading workload and does not
  establish world performance or resolution.

Both runs exited normally, reported no log loss or frame-sequence gaps, and
preserved original-file integrity. These are different workloads, not a
controlled comparison. Raw logs and derived evidence remain outside Git in
the adjacent `runtime/starport/runs` directory. The earlier sections record
the checks and visual observations available at their respective stages.
