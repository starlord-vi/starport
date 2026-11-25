# Offline checks and user-run verification

Do not use an automated game launch. The user runs the comparisons below.
Keep the same save, camera, cache and workload for each comparison. Compare
world-only windows; menu FPS does not establish gameplay performance.

## Task 1: attribution baseline

From PowerShell in the source repository:

```powershell
$env:STARPORT_TELEMETRY = '1' # default on; 0 disables the new collector
.\Play-Starport.cmd --console series-x --skip-top-mip
```

Every roughly two seconds of presents, `console.log` includes:

- `starport::telemetry`: one sampled presented frame's CPU phase times,
  actual outstanding submission flights, live texture allocation bytes,
  waiting descriptor slots, residency evictions and cumulative descriptor
  publications. Publication count includes normal writes, not only eviction.
- `starport::resource`: top 20 upload/readback attribution entries for that
  sampled frame, identified by allocation ID, guest address, dimensions and
  format (format 0 denotes an untyped buffer). A mixed first/repeated upload
  can occupy two reason entries. Overflow bytes are explicitly counted when
  more than 4,096 attribution entries occur in one frame.
- Upload totals distinguish first upload, dirty reupload and initial mip
  rebasing; demotion is reserved for the later resource replacement path.
  Unique bytes mean union of byte offsets copied at least once per host
  allocation, not content hashes or unique pixel values. Texture values
  include copy-footprint padding. Buffer values count CPU-shadow refresh
  traffic; this does not imply every byte crossed PCIe in a GPU upload.
- Readback reasons distinguish explicit guest texture-to-buffer copies,
  CPU merge scans of shader-written buffers and fetched GPU dirty pieces.
  Scan bytes are not PCIe bytes or necessarily bytes written back to RAM.
- `starport::refused` counts guarded compute *recording attempts*, not shader
  execution. Recording can precede submission; zero counts do not prove that
  all indirect dispatches are safe or implemented.
- `ambient_hits` counts the existing indirect-term override hook. Debug-level
  `starport::ambient` identifies its output resource. It does not establish
  a downstream albedo multiply or prove the diffuse composite is correct.

The per-frame bucket is reset on every present; only a sample is logged.
Phase measurements are CPU wall time, including fence waits, not GPU execution
timestamps. The existing log-loss counters must remain zero for comparisons.

Further task switches and controlled comparisons will be documented here as
their implementations and hazard checks are completed.
