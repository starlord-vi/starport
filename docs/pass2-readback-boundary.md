# Readback protection conformance: second pass

These are native Windows fixtures, not a working production lazy-readback
implementation. The game was not launched. GPU data in the alias fixture is
explicitly mocked as already complete.

`cargo test -p victory-gpu --all-features readback_probe -- --test-threads=1 --nocapture`
passed both tests on this machine:

- Private MEM_WRITE_WATCH memory retains native write watching while PAGE_GUARD
  and PAGE_NOACCESS read faults are handled. A later write is still reported.
- Two views of a reserved pagefile section permit writing through a host-only
  alias while the guest page remains PAGE_NOACCESS. Only the faulted page is
  published; the other guest page remains inaccessible and its bytes untouched.
  Native GetWriteWatch rejects these mapped views (observed status 0xffffffff),
  so an alias-backed allocator requires a separate software dirty journal.

Thus protection/write-watch composition itself is not the private-memory
blocker. Publication is: ordinary private VirtualAlloc pages have no writable
host-only alias. Temporarily changing a protected guest page to writable before
merging permits another guest reader to observe an incomplete/stale page.
PAGE_GUARD also clears on the first access before the handler finishes, so it
does not by itself hold other readers out. Read-only protection traps writes,
not reads, and cannot implement first-read demand paging alone.

Production integration still needs a managed alias/lifetime registry covering
all guest mappings, software write tracking where native write-watch is absent,
kernel-I/O observation, immutable per-fence output versions and a fault worker
that never waits for a lock held by its faulting thread. Replacing private
allocations with mapped sections also changes VirtualFree/decommit/query
semantics; that cannot be hidden by merely installing a vectored handler.
Existing native private allocations and physical-page views retain their
current behavior. No no-op `STARPORT_READBACK=lazy` switch is advertised.

Until those contracts are complete, buffer CPU visibility remains eager and
buffer-output submissions remain synchronous. The self-write compare filter
is independent and implemented; it never clears a concurrent guest write with
a post-merge ResetWriteWatch. The new timers and bench expose whether that and
texture residency changes improve the actual scene.

Microsoft documents [the one-shot guard behavior](https://learn.microsoft.com/en-us/windows/win32/memory/creating-guard-pages)
and [native write-watch lifetime](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc).
