//! 録画中にシステムカーソルを隠した際に表示する代理十字カーソル。
//! WDA_EXCLUDEFROMCAPTURE によりキャプチャには映らない。

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 十字カーソルのサイズ (片腕の長さ)
const ARM_LEN: i32 = 15;
/// 線の太さ
const PEN_WIDTH: i32 = 2;
/// ウィンドウ全体サイズ
const WIN_SIZE: i32 = ARM_LEN * 2 + 1;
/// 十字線の色 (BGR: 赤)
const CROSS_COLOR: u32 = 0x000000FF;
/// 透過キー色 (BGR: マゼンタ)
const KEY_COLOR: u32 = 0x00FF00FF;

static mut KEY_BRUSH: HBRUSH = HBRUSH(std::ptr::null_mut());

/// キャプチャ非対象の十字カーソルオーバーレイ。
pub struct CursorProxy {
    hwnd: HWND,
    visible: bool,
}

impl CursorProxy {
    pub fn new() -> Self {
        unsafe { Self::init() }
    }

    unsafe fn init() -> Self {
        unsafe {
            let class_name = w!("AynimeCursorProxy");

            if KEY_BRUSH.0.is_null() {
                KEY_BRUSH = CreateSolidBrush(COLORREF(KEY_COLOR));
            }

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(cursor_wnd_proc),
                hInstance: HINSTANCE::default(),
                hbrBackground: KEY_BRUSH,
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);

            // WS_EX_TRANSPARENT は除外 — レイヤードウィンドウの描画を妨げるため
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                class_name,
                w!(""),
                WS_POPUP,
                0,
                0,
                WIN_SIZE,
                WIN_SIZE,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            // マゼンタを透過キーに設定
            let _ = SetLayeredWindowAttributes(
                hwnd,
                COLORREF(KEY_COLOR),
                0,
                LAYERED_WINDOW_ATTRIBUTES_FLAGS(0x01), // LWA_COLORKEY
            );

            // キャプチャから除外
            let _ = crate::overlay::window::set_exclude_from_capture(hwnd);

            Self {
                hwnd,
                visible: false,
            }
        }
    }

    /// 代理カーソルを表示する。
    pub fn show(&mut self) {
        if self.visible {
            return;
        }
        self.visible = true;
        self.update_position();
        log::info!("Cursor proxy shown");
    }

    /// 代理カーソルを非表示にする。
    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.visible = false;
        log::info!("Cursor proxy hidden");
    }

    /// マウス位置に追従させる。録画ループ内で毎フレーム呼ぶ。
    pub fn update_position(&self) {
        if !self.visible {
            return;
        }
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    pt.x - ARM_LEN,
                    pt.y - ARM_LEN,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
                let _ = InvalidateRect(Some(self.hwnd), None, true);
            }
        }
    }
}

impl Drop for CursorProxy {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn cursor_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        // クリックを下のウィンドウに透過させる
        if msg == WM_NCHITTEST {
            return LRESULT(-1); // HTTRANSPARENT
        }

        if msg == WM_PAINT {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let pen = CreatePen(PS_SOLID, PEN_WIDTH, COLORREF(CROSS_COLOR));
            let old_pen = SelectObject(hdc, pen.into());

            let c = ARM_LEN; // 中心座標
            // 横線
            let _ = MoveToEx(hdc, 0, c, None);
            let _ = LineTo(hdc, WIN_SIZE, c);
            // 縦線
            let _ = MoveToEx(hdc, c, 0, None);
            let _ = LineTo(hdc, c, WIN_SIZE);

            SelectObject(hdc, old_pen);
            let _ = DeleteObject(pen.into());
            let _ = EndPaint(hwnd, &ps);
            return LRESULT(0);
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
