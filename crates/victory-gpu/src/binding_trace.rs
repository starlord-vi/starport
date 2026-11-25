//! Bounded potential dataflow from guarded compute bindings. No shader is run
//! or resource contents read. Static bindings are not proof of dynamic access.
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Key {
    Object(usize),
    Memory(u64, u64),
}
fn overlaps(a: Key, b: Key) -> bool {
    match (a, b) {
        (Key::Object(a), Key::Object(b)) => a == b,
        (Key::Memory(a, n), Key::Memory(b, m)) => {
            a < b.saturating_add(m.max(1)) && b < a.saturating_add(n.max(1))
        }
        _ => false,
    }
}
struct Trace {
    headers: BTreeSet<(String, bool, bool)>,
    bindings: BTreeSet<(String, u8, String, u32, Option<Key>)>,
    writes: Vec<(String, Key)>,
    edges: BTreeSet<(String, String, Key)>,
}
static TRACE: LazyLock<Mutex<Trace>> = LazyLock::new(|| {
    Mutex::new(Trace {
        headers: BTreeSet::new(),
        bindings: BTreeSet::new(),
        writes: Vec::new(),
        edges: BTreeSet::new(),
    })
});
static WATCHING: AtomicBool = AtomicBool::new(false);
pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_WAVE64_TRACE").is_none_or(|v| v != "0"))
}
pub(crate) fn watching() -> bool {
    enabled() && WATCHING.load(Relaxed)
}
pub(crate) fn begin(hash: &str, groups: Option<[u32; 3]>, ready: bool) {
    if !enabled() {
        return;
    }
    let fresh = TRACE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .headers
        .insert((hash.into(), ready, groups.is_none()));
    if fresh {
        tracing::info!(target:"starport::wave64",hash,?groups,metadata_ready=ready,
        executed=false,"guarded compute binding audit; Wave64 guard retained");
    }
}
pub(crate) fn binding(hash: &str, slot: u8, kind: &str, root: &str, offset: u32, key: Option<Key>) {
    if !enabled() {
        return;
    }
    let fresh = {
        let mut trace = TRACE.lock().unwrap_or_else(|e| e.into_inner());
        if kind == "Uav"
            && let Some(key) = key
            && trace.writes.len() < 256
            && !trace.writes.iter().any(|(h, k)| h == hash && *k == key)
        {
            trace.writes.push((hash.into(), key));
            WATCHING.store(true, Relaxed);
        }
        trace.bindings.len() < 1024
            && trace
                .bindings
                .insert((hash.into(), slot, kind.into(), offset, key))
    };
    if fresh {
        tracing::info!(target:"starport::wave64",hash,slot,kind,root,offset_dwords=offset,resource=?key,
        potential_write=kind=="Uav",executed=false,"guarded shader binding; unresolved or dynamic indices remain unknown");
    }
}
pub(crate) fn consumer(shader: &str, key: Key) {
    if !watching() {
        return;
    }
    let fresh = {
        let mut trace = TRACE.lock().unwrap_or_else(|e| e.into_inner());
        if trace.edges.len() >= 1024 {
            return;
        }
        let sources: Vec<_> = trace
            .writes
            .iter()
            .filter(|(_, written)| overlaps(*written, key))
            .map(|(hash, _)| hash.clone())
            .collect();
        sources
            .into_iter()
            .filter(|source| trace.edges.insert((source.clone(), shader.into(), key)))
            .collect::<Vec<_>>()
    };
    for producer in fresh {
        tracing::info!(target:"starport::wave64",producer,consumer=shader,resource=?key,
            "later bound shader may read a guarded output; static binding overlap, not an executed-read proof");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binding_overlap_respects_object_identity_and_memory_alias_ranges() {
        assert!(overlaps(Key::Object(3), Key::Object(3)));
        assert!(!overlaps(Key::Object(3), Key::Object(4)));
        assert!(overlaps(Key::Memory(100, 20), Key::Memory(110, 20)));
        assert!(!overlaps(Key::Memory(100, 20), Key::Memory(120, 20)));
        assert!(overlaps(Key::Memory(100, 0), Key::Memory(100, 0)));
        assert!(!overlaps(Key::Object(100), Key::Memory(100, 20)));
    }
}
