//! Starport keyboard/mouse adapter. GPL-3.0-or-later.
//! F6 captures/releases raw relative mouse input for the virtual pad.
//! Losing focus always releases the cursor and resets buttons and movement.

use std::sync::{LazyLock, Mutex};
use std::time::Instant;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTHEADER, RID_INPUT, RIM_TYPEMOUSE, MOUSE_MOVE_ABSOLUTE,
};
use windows_sys::Win32::UI::WindowsAndMessaging as wm;
use super::PadState;

#[derive(Default)]
struct Motion {
    enabled: bool,
    dx: i64,
    dy: i64,
    left: bool,
    right: bool,
}

impl Motion {
    fn packet(&mut self, dx: i32, dy: i32, flags: u16) {
        if !self.enabled { return; }
        self.dx = self.dx.saturating_add(i64::from(dx));
        self.dy = self.dy.saturating_add(i64::from(dy));
        if flags & 1 != 0 { self.left = true; }
        if flags & 2 != 0 { self.left = false; }
        if flags & 4 != 0 { self.right = true; }
        if flags & 8 != 0 { self.right = false; }
    }

    fn take(&mut self, seconds: f32) -> PadState {
        if !self.enabled { return PadState::default(); }
        // Translate relative counts/sec into a bounded virtual right stick.
        // The title still applies its controller sensitivity and acceleration.
        let scale = (seconds.max(0.001) * 900.0).max(1.0);
        let pad = PadState {
            right_x: (self.dx as f32 / scale).clamp(-1.0, 1.0),
            right_y: (-self.dy as f32 / scale).clamp(-1.0, 1.0),
            right_trigger: if self.left { 1.0 } else { 0.0 },
            left_trigger: if self.right { 1.0 } else { 0.0 },
            ..Default::default()
        };
        self.dx = 0;
        self.dy = 0;
        pad
    }
}

static MOTION: Mutex<Motion> = Mutex::new(Motion {
    enabled: false, dx: 0, dy: 0, left: false, right: false,
});
static LAST: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

pub(super) fn sample(focused: bool) -> PadState {
    if !focused { release(); }
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let elapsed = last.elapsed().as_secs_f32();
    *last = Instant::now();
    let mut motion = MOTION.lock().unwrap_or_else(|e| e.into_inner());
    motion.take(elapsed)
}

pub(crate) fn register(hwnd: HWND) {
    let device = RAWINPUTDEVICE {
        usUsagePage: 1, usUsage: 2, dwFlags: 0, hwndTarget: hwnd,
    };
    // SAFETY: one initialized device; foreground-only, no global input sink.
    if unsafe { RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) } == 0 {
        tracing::warn!("mouse registration failed; keyboard controls remain available");
    }
}

fn release() {
    let owned = {
        let mut motion = MOTION.lock().unwrap_or_else(|e| e.into_inner());
        let owned = motion.enabled;
        *motion = Motion::default();
        owned
    };
    if !owned { return; }
    // SAFETY: release our confinement and restore a standard cursor.
    unsafe {
        wm::ClipCursor(std::ptr::null());
        wm::SetCursor(wm::LoadCursorW(std::ptr::null_mut(), wm::IDC_ARROW));
    }
}

/// Returns true for messages consumed by the adapter. WM_INPUT is forwarded
/// to the title/DefWindowProc so Windows can clean up the input handle.
pub(crate) fn message(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> bool {
    if msg == wm::WM_KILLFOCUS || msg == wm::WM_NCDESTROY
        || (msg == wm::WM_ACTIVATEAPP && wp == 0) {
        release();
    }
    if msg == wm::WM_KEYDOWN && wp == 0x75 && (lp as usize & (1 << 30)) == 0 {
        let enabled = MOTION.lock().unwrap_or_else(|e| e.into_inner()).enabled;
        if enabled { release(); } else {
            let mut rect = RECT::default();
            // SAFETY: a title window we subclassed; convert its client rectangle
            // to screen coordinates, and capture only if it owns foreground.
            let captured = unsafe {
                if wm::GetForegroundWindow() != hwnd || wm::GetClientRect(hwnd, &mut rect) == 0 {
                    false
                } else {
                    let mut points = [
                        windows_sys::Win32::Foundation::POINT { x: rect.left, y: rect.top },
                        windows_sys::Win32::Foundation::POINT { x: rect.right, y: rect.bottom },
                    ];
                    ClientToScreen(hwnd, &mut points[0]) != 0
                        && ClientToScreen(hwnd, &mut points[1]) != 0
                        && { rect = RECT { left: points[0].x, top: points[0].y,
                            right: points[1].x, bottom: points[1].y };
                            wm::ClipCursor(&rect) != 0 }
                }
            };
            if captured {
                MOTION.lock().unwrap_or_else(|e| e.into_inner()).enabled = true;
                // SAFETY: hide for our captured client area only.
                unsafe { wm::SetCursor(std::ptr::null_mut()); }
            }
        }
        let captured = MOTION.lock().unwrap_or_else(|e| e.into_inner()).enabled;
        tracing::info!(captured, "F6 mouse capture toggled");
        return true;
    }
    if msg == wm::WM_SETCURSOR && (lp as u16) == wm::HTCLIENT as u16
        && MOTION.lock().unwrap_or_else(|e| e.into_inner()).enabled {
        // SAFETY: our client area, while mouse capture is enabled.
        unsafe { wm::SetCursor(std::ptr::null_mut()); }
        return true;
    }
    if msg == wm::WM_INPUT {
        let mut raw = RAWINPUT::default();
        let mut bytes = size_of::<RAWINPUT>() as u32;
        // SAFETY: aligned RAWINPUT storage, exact capacity and header size.
        let read = unsafe { GetRawInputData(lp as _, RID_INPUT, (&raw mut raw).cast(),
            &mut bytes, size_of::<RAWINPUTHEADER>() as u32) };
        if read != u32::MAX && read as usize >= size_of::<RAWINPUTHEADER>()
            + size_of::<windows_sys::Win32::UI::Input::RAWMOUSE>()
            && raw.header.dwType == RIM_TYPEMOUSE {
            // SAFETY: dwType identifies the mouse union member.
            let mouse = unsafe { raw.data.mouse };
            if mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0 {
                // SAFETY: RAWMOUSE's documented button flags union member.
                let flags = unsafe { mouse.Anonymous.Anonymous.usButtonFlags };
                MOTION.lock().unwrap_or_else(|e| e.into_inner())
                    .packet(mouse.lLastX, mouse.lLastY, flags);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_motion_is_consumed_once_and_is_bounded() {
        let mut m = Motion { enabled: true, ..Default::default() };
        m.packet(9, -9, 0);
        let p = m.take(0.01);
        assert_eq!((p.right_x, p.right_y), (1.0, 1.0));
        assert_eq!(m.take(0.01), PadState::default());
        m.packet(i32::MAX, i32::MIN, 0);
        let p = m.take(0.01);
        assert_eq!((p.right_x, p.right_y), (1.0, 1.0));
    }

    #[test]
    fn buttons_persist_until_up_and_disabled_input_is_neutral() {
        let mut m = Motion { enabled: true, ..Default::default() };
        m.packet(0, 0, 1 | 4);
        for _ in 0..2 {
            let p = m.take(0.01);
            assert_eq!((p.left_trigger, p.right_trigger), (1.0, 1.0));
        }
        m.packet(0, 0, 2 | 8);
        assert_eq!(m.take(0.01), PadState::default());
        m = Motion::default();
        m.packet(100, 100, 1 | 4);
        assert_eq!(m.take(0.01), PadState::default());
    }
}
