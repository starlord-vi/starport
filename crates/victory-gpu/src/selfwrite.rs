//! Filter dirty notices against the host's current merged bytes. Do not reset
//! write-watch after comparison: native guest threads are not quiescent.
use std::cell::RefCell;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Reset,
    Compare,
    Off,
}
fn parse(value: &str) -> Option<Mode> {
    match value {
        "reset" => Some(Mode::Reset),
        "compare" => Some(Mode::Compare),
        "off" => Some(Mode::Off),
        _ => None,
    }
}
pub(crate) fn enabled() -> bool {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| {
        let requested=std::env::var("STARPORT_SELFWRITE_FILTER").unwrap_or_else(|_|"reset".into());
        let mode=parse(&requested).unwrap_or_else(|| {
            tracing::warn!(%requested,"invalid self-write mode; using off");Mode::Off
        });
        tracing::info!(target:"starport::selfwrite",?mode,effective=if mode==Mode::Off {"off"} else {"compare"},
            "reset requests use compare safety net: no guest-thread quiescence exists; write-watch is never reset after the source snapshot");
        mode
    })!=Mode::Off
}
pub(crate) const BATCH: usize = 256 << 10;
thread_local! {static SCRATCH:RefCell<Vec<u8>>=const {RefCell::new(Vec::new())};}

/// One bounded snapshot read amortizes kernel-call cost over adjacent dirty
/// pages; comparison still selects individual 4 KiB pages inside that snapshot.
pub(crate) fn snapshot<R>(source: usize, len: usize, consume: impl FnOnce(&[u8]) -> R) -> R {
    assert!(len <= BATCH);
    SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.resize(len, 0);
        crate::host::copy_mapped(scratch.as_mut_ptr(), source as *const u8, len, false);
        consume(&scratch)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_have_an_explicit_kill_switch_and_no_implicit_reset() {
        assert_eq!(parse("reset"), Some(Mode::Reset));
        assert_eq!(parse("compare"), Some(Mode::Compare));
        assert_eq!(parse("off"), Some(Mode::Off));
        assert_eq!(parse("fast"), None);
    }
}
