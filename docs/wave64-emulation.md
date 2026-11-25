# Wave64 compute: diagnostics and a future implementation

All four programs remain guarded on the Wave32-only NVIDIA device. Offline
DXIL validation succeeds, but creating their compute PSOs is still refused.
The builder's all-or-none guard stays intact. This patch adds no Wave64
emulation and does not remove wave-size metadata.

`STARPORT_WAVE64_TRACE=1` (default; `0` disables) records guarded direct and
indirect dispatch attempts. Resource-use metadata is read from the native
container without invoking shader translation or making a PSO. Static root
views, explicit table descriptors and placed-memory views can identify
potential output resources. Later bound shaders with matching object IDs or
overlapping memory spans produce potential consumer edges.

These are **static binding observations**, not GPU executions or proofs that
a shader reads a particular index. UAVs may be read-only in a branch or only
written. Dynamic heap indices/root constants can remain unresolved. Indirect
group counts are not read from GPU memory for diagnostics. Aliases through
different resource objects can be missed, and reused addresses can produce
false edges. Histories are bounded to 256 potential writes and 1,024 binding
records/edges; an absent edge is never proof of absence of a dependency.

## Per-shader starting points

The following counts describe calls/declarations in the locally translated
DXIL inspected offline, not the original instruction count or runtime cost.
No proprietary shader fixtures or disassembly are stored in this repository.

| Shader hash | Offline observation | Proposed next step |
| --- | --- | --- |
| `2da4beb386509b77` | 38 lane-at exchanges, 1 first-lane read, 1 barrier call, shared-memory declaration | Audit every exchange's source lane/active mask. Prototype 64-lane register exchange in a uniform software-wave phase; validate both halves and inactive sources before integrating LDS ordering. |
| `369cbe906ea72764` | No wave-intrinsic calls or barrier calls in the inspected DXIL; shared-memory declaration remains | First establish whether shared memory is live and whether lane/thread indexing or original execution ordering needs 64 lanes. A declaration alone proves neither requirement nor safety. Keep guarded until equivalence or a required synchronization schedule is demonstrated. |
| `02384a60fda6f2d0` | 13 first-lane reads, 6 lane-at exchanges, 5 barrier calls, shared-memory declaration | Model the active 64-bit mask and reconvergence explicitly. Validate first-active-lane selection across the two NVIDIA halves and prove every existing barrier is reached uniformly. |
| `6b82bc7d22016cb5` | 9 first-lane reads, 3 barrier calls, shared-memory declaration | Isolate first-active-lane and LDS publication phases. Validate masks with an empty low half, partial high half and divergent exits; only then test the ordered builder resources. |

## Possible synchronization model

A logical console wave keeps 64 lane register values and a 64-bit active mask.
NVIDIA executes two 32-lane halves. Values needed across halves can be staged
in separate emulation scratch memory. First-lane operations select from the
entire logical mask; an ordinary host WaveReadLaneFirst is insufficient.

Barriers must occur at uniform host control-flow points. Putting a group
barrier directly inside a divergent guest branch can deadlock or be undefined.
A correct general lowering needs an explicit program-counter/reconvergence
state machine with predicated effects and uniform rendezvous, or multiple
ordered kernels separated at proven synchronization boundaries. Workgroups
with several logical waves also need to coordinate group-wide barriers and
early exits. Scratch must not alias the program's LDS, and combined storage
must fit actual host limits. Atomics and memory scope/order require separate
validation; kernel splitting cannot invent cross-group synchronization.

Start with synthetic two-half tests: exchange from each half, inactive sources,
divergent loops/exits, first-active-lane masks, LDS producer/consumer ordering,
barriers, and UAV canaries. Then validate each shader's host DXIL and PSO and
compare properties of its locally supplied inputs/outputs against a valid
reference. Finally check the entire builder graph: fixing one stage cannot
enable the others while any required stage still fails. The separate divergent
lane-mask translation failure remains another unresolved issue.

Microsoft's [wave-size specification](https://microsoft.github.io/DirectX-Specs/d3d/HLSL_SM_6_6_WaveSize.html)
explains why unsupported requirements reject PSO creation. Its
[group barrier contract](https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/groupmemorybarrierwithgroupsync)
requires uniform participation. No claim about emulation FPS follows from
these static operation counts.
