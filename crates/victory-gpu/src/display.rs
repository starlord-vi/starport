//! Shared ceiling for title modes, guest windows and presentation.
use std::sync::OnceLock;

fn parse(value: &str) -> Option<(u32, u32)> {
    let (w, h) = value.trim().split_once(['x', 'X'])?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    ((1..=16384).contains(&w) && (1..=16384).contains(&h)).then_some((w, h))
}

fn choose(value: Option<&str>, monitor: (u32, u32)) -> Option<(u32, u32)> {
    if value == Some("0") {
        None
    } else {
        Some(value.and_then(parse).unwrap_or(monitor))
    }
}

/// Unset: primary monitor; WxH: explicit ceiling; 0: uncapped legacy behavior.
pub fn limit() -> Option<(u32, u32)> {
    static LIMIT: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        let monitor = primary();
        let asked = std::env::var("STARPORT_MAX_RES").ok().filter(|v| !v.trim().is_empty());
        if let Some(v) = asked.as_deref() && v != "0" && parse(v).is_none() {
            tracing::warn!(value=v, "invalid STARPORT_MAX_RES; using primary monitor");
        }
        let ceiling = choose(asked.as_deref(), monitor);
        tracing::info!(target: "starport::display", monitor_width=monitor.0, monitor_height=monitor.1,
            max_resolution=?ceiling, "chosen display ceiling; STARPORT_MAX_RES=0 disables");
        ceiling
    })
}

#[cfg(windows)]
fn primary() -> (u32, u32) {
    use windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: null selects the primary display; output is correctly sized.
    if unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool()
        && mode.dmPelsWidth > 0
        && mode.dmPelsHeight > 0
    {
        return (mode.dmPelsWidth, mode.dmPelsHeight);
    }
    // SAFETY: plain system metric queries, no pointers.
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    if w > 0 && h > 0 {
        (w as u32, h as u32)
    } else {
        tracing::warn!("monitor query unavailable; using conservative 1920x1080 ceiling");
        (1920, 1080)
    }
}
#[cfg(not(windows))]
fn primary() -> (u32, u32) {
    (1920, 1080)
}

/// Aspect-preserving integer fit; never enlarges a valid source.
pub fn fit(size: (u32, u32), maximum: Option<(u32, u32)>) -> (u32, u32) {
    let (w, h) = size;
    let Some((mw, mh)) = maximum else {
        return size;
    };
    if w == 0 || h == 0 || mw == 0 || mh == 0 {
        return (1, 1);
    }
    if w <= mw && h <= mh {
        return size;
    }
    if u64::from(w) * u64::from(mh) > u64::from(h) * u64::from(mw) {
        (
            mw.min(w),
            ((u64::from(h) * u64::from(mw) / u64::from(w)) as u32)
                .max(1)
                .min(mh),
        )
    } else {
        (
            ((u64::from(w) * u64::from(mh) / u64::from(h)) as u32)
                .max(1)
                .min(mw),
            mh.min(h),
        )
    }
}
pub fn clamp(size: (u32, u32)) -> (u32, u32) {
    fit(size, limit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mocked_monitor_and_explicit_ceiling() {
        assert_eq!(choose(None, (1920, 1080)), Some((1920, 1080)));
        assert_eq!(choose(Some("1280x720"), (1920, 1080)), Some((1280, 720)));
        assert_eq!(choose(Some("0"), (1920, 1080)), None);
        for bad in ["4k", "0x1080", "-1x720", "999999x999999"] {
            assert_eq!(choose(Some(bad), (1920, 1080)), Some((1920, 1080)));
        }
    }
    #[test]
    fn clamp_landscape_portrait_and_large_sizes() {
        assert_eq!(fit((3840, 2160), Some((1920, 1080))), (1920, 1080));
        assert_eq!(fit((2560, 1440), Some((1920, 1080))), (1920, 1080));
        assert_eq!(fit((1080, 1920), Some((1920, 1080))), (607, 1080));
        assert_eq!(fit((1280, 720), Some((1920, 1080))), (1280, 720));
        assert_eq!(fit((u32::MAX, u32::MAX), Some((1920, 1080))), (1080, 1080));
        assert_eq!(fit((3840, 2160), None), (3840, 2160));
    }
}
