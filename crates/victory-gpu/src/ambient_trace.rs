//! Observes the existing override without changing its shader or scheduling.
use std::sync::{LazyLock, Mutex, OnceLock};

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("STARPORT_AMBIENT_TRACE").is_none_or(|v| v != "0"))
}

#[derive(Default)]
struct Frame {
    matched_resolves: u64,
    override_hits: u64,
    first: Option<(String, u64, i32, i32)>,
}
impl Frame {
    fn record(&mut self, hit: bool, hash: &str, resource: u64, format: i32, dimension: i32) {
        self.matched_resolves += 1;
        self.override_hits += u64::from(hit);
        if self.first.is_none() {
            self.first = Some((hash.to_owned(), resource, format, dimension));
        }
    }
}
static FRAME: LazyLock<Mutex<Frame>> = LazyLock::new(|| Mutex::new(Frame::default()));

pub(crate) fn record(hit: bool, hash: &str, resource: u64, format: i32, dimension: i32) {
    if enabled() {
        FRAME
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(hit, hash, resource, format, dimension);
    }
}

pub(crate) fn present() {
    if !enabled() {
        return;
    }
    let sample = std::mem::take(&mut *FRAME.lock().unwrap_or_else(|e| e.into_inner()));
    let (root, offset) = crate::ambient::LIGHT;
    tracing::info!(target: "starport::ambient", frame=crate::frame::current(),
        matched_resolves=sample.matched_resolves, override_hits=sample.override_hits,
        first_binding=?sample.first, root, offset_dwords=offset, level=?crate::ambient::level(),
        downstream_albedo_multiply="unknown",
        "presented-frame ambient audit; first_binding=(shader, output, format, dimension); hook reach does not establish diffuse composite correctness");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_distinguishes_matched_bindings_from_writes_and_resets() {
        let mut frame = Frame::default();
        frame.record(false, "disabled", 10, 42, 4);
        frame.record(true, "active", 20, 42, 4);
        assert_eq!(frame.matched_resolves, 2);
        assert_eq!(frame.override_hits, 1);
        assert_eq!(frame.first, Some(("disabled".into(), 10, 42, 4)));
        let previous = std::mem::take(&mut frame);
        assert_eq!(previous.override_hits, 1);
        assert_eq!(frame.matched_resolves, 0);
        assert!(frame.first.is_none());
    }
}
