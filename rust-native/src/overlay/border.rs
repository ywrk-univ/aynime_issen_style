//! Persistent region border overlay using 4 thin Win32 windows.
//! Mouse-passthrough, always-on-top, excluded from capture.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const BORDER_THICKNESS: i32 = 3;
const BORDER_COLOR: u32 = 0x00D77800; // BGR for #0078D7

static mut BORDER_BRUSH: HBRUSH = HBRUSH(std::ptr::null_mut());

/// Manages 4 thin windows forming a rectangular border.
pub struct RegionBorder {
    windows: [HWND; 4], // top, bottom, left, right
    visible: bool,
}

impl RegionBorder {
    pub fn new() -> Self {
        unsafe { Self::init() }
    }

    unsafe fn init() -> Self {
        unsafe {
            // Register class once
            let class_name = w!("AynimeBorderOverlay");

            if BORDER_BRUSH.0.is_null() {
                BORDER_BRUSH = CreateSolidBrush(COLORREF(BORDER_COLOR));
            }

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(border_wnd_proc),
                hInstance: HINSTANCE::default(),
                hbrBackground: BORDER_BRUSH,
                lpszClassName: class_name,
                ..Default::default()
            };
            // Ignore error if already registered
            let _ = RegisterClassExW(&wc);

            let mut windows = [HWND::default(); 4];
            for i in 0..4 {
                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                    class_name,
                    w!(""),
                    WS_POPUP,
                    0, 0, 0, 0,
                    None, None, None, None,
                )
                .unwrap();

                // Exclude from screen capture
                let _ = crate::overlay::window::set_exclude_from_capture(hwnd);
                windows[i] = hwnd;
            }

            Self {
                windows,
                visible: false,
            }
        }
    }

    /// Update the border position/size to match the given region (physical pixels).
    /// Shows the border if hidden.
    pub fn update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        unsafe { self.update_impl(x, y, w, h) }
    }

    unsafe fn update_impl(&mut self, x: i32, y: i32, w: i32, h: i32) {
        unsafe {
            let t = BORDER_THICKNESS;

            // Positions: [top, bottom, left, right]
            let positions: [(i32, i32, i32, i32); 4] = [
                (x - t, y - t, w + 2 * t, t),         // top
                (x - t, y + h, w + 2 * t, t),         // bottom
                (x - t, y, t, h),                       // left
                (x + w, y, t, h),                       // right
            ];

            for (i, &(px, py, pw, ph)) in positions.iter().enumerate() {
                let _ = SetWindowPos(
                    self.windows[i],
                    Some(HWND_TOPMOST),
                    px, py, pw, ph,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }

            self.visible = true;
        }
    }

    /// Hide the border windows.
    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        unsafe {
            for hwnd in &self.windows {
                let _ = ShowWindow(*hwnd, SW_HIDE);
            }
        }
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

impl Drop for RegionBorder {
    fn drop(&mut self) {
        unsafe {
            for hwnd in &self.windows {
                let _ = DestroyWindow(*hwnd);
            }
        }
    }
}

unsafe extern "system" fn border_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
