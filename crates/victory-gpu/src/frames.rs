//! Bounded presentation flights. Guest-buffer outputs still complete
//! synchronously until demand readback has a sound CPU read barrier.
use std::sync::OnceLock;

fn parse(value: Option<&str>) -> usize {
    match value {
        None => 2,
        Some(v) => v.parse().ok().filter(|n| (1..=4).contains(n)).unwrap_or(1),
    }
}
pub(crate) fn limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        let value=std::env::var("STARPORT_FRAMES_IN_FLIGHT").ok();let n=parse(value.as_deref());
        tracing::info!(target:"starport::frames",limit=n,
            "bounded presentation/no-guest-output flights; guest-buffer outputs remain synchronous; 1 restores serialization");n
    })
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Reuse(usize),
    Grow,
    Wait(u64),
}
pub(crate) fn slot(fences: &[u64], finished: u64, limit: usize) -> Slot {
    if let Some(i) = fences.iter().position(|f| *f <= finished) {
        return Slot::Reuse(i);
    }
    if fences.len() < limit {
        return Slot::Grow;
    }
    Slot::Wait(*fences.iter().min().unwrap_or(&0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_never_reuses_an_unfinished_slot_or_grows_past_the_limit() {
        assert_eq!(slot(&[], 0, 2), Slot::Grow);
        assert_eq!(slot(&[2, 4], 1, 2), Slot::Wait(2));
        assert_eq!(slot(&[2, 4], 2, 2), Slot::Reuse(0));
        assert_eq!(slot(&[2, 4], 1, 3), Slot::Grow);
        assert_eq!(slot(&[5, 2], 1, 2), Slot::Wait(2));
        assert_eq!(slot(&[5], 0, 1), Slot::Wait(5));
    }
    #[test]
    fn frame_configuration_has_a_serial_kill_switch() {
        assert_eq!(parse(None), 2);
        assert_eq!(parse(Some("1")), 1);
        assert_eq!(parse(Some("4")), 4);
        for bad in ["0", "5", "999999", "auto"] {
            assert_eq!(parse(Some(bad)), 1);
        }
    }
}
