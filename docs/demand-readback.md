# Demand readback: implementation boundary

Arbitrary first-read demand paging is not implemented. The inherited copy-back
policy remains active; no output is discarded based on an incomplete consumer
allowlist. No new `STARPORT_READBACK` mode is claimed working yet.

The guest runs native CPU instructions. Its GPU addresses are CPU addresses,
so it can read an output without `Map`, a query, or an intercepted HLE call.
An allowlist based only on known API consumers would silently return stale
bytes for those reads. Logs can establish observed consumers, but cannot prove
the absence of an unobserved CPU consumer. That includes indirect arguments
interpreted by the host and physically aliased page-pool memory.

Existing GPU-local dirty-piece fetches, explicit texture-to-buffer copies and
byte-wise CPU merge rules remain intact. A GPU write must not overwrite
unrelated CPU edits in the same page. Source/destination lifetime and merge
ordering must also survive asynchronous submission and remapping.

## Required before skipped outputs can be enabled

1. Cover all guest aliases of a protected page, including physical page-pool
   views and kernel I/O. An API observation does not prove another alias has
   no CPU consumer.
2. Keep a host-only writable alias or equivalent mechanism while guest aliases
   are inaccessible. Temporarily opening a guest page to merge data lets
   another guest thread read stale bytes during that interval.
3. Associate each protected output version with its fence and merge snapshot.
   A fault must wait for that version, merge only changed bytes, then publish
   accessibility atomically to readers.
4. Avoid recursion when host preparation reads guest metadata; do not acquire
   a submission/mapping lock already held by the faulting thread. A vectored
   handler must not allocate under arbitrary guest-held locks or suspend a
   thread that owns a lock required by copy-back.
5. Cover unmap/remap, decommit, free, partial kernel writes and simultaneous
   reads with real Windows mapping tests, including multiple aliases.

The inherited in-place heap experiment avoids copies for some private memory,
but pins pages, excludes reserved section views and has additional asynchronous
hazards. It is not enabled as a replacement for demand readback. The requested
demand-default mode is deferred until this CPU-memory read-barrier machinery
exists; the current changes do not claim to eliminate the measured merge cost.
