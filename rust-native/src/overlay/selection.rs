//! Native Win32 + GDI screen region selection overlay.
//!
//! Creates a fullscreen borderless window, BitBlts the desktop as background,
//! and lets the user drag-select a region. Pure Win32 — no egui, no GL.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Result of the selection operation.
#[derive(Debug, Clone)]
pub struct SelectionResult {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// If capture_on_select was true, contains RGBA pixel data of the selected region
    /// extracted from the frozen screenshot (the frame at selection start).
    pub rgba_data: Option<Vec<u8>>,
}

struct SelectionData {
    mem_dc: HDC,
    mem_bmp: HBITMAP,
    old_bmp: HGDIOBJ,
    screen_w: i32,
    screen_h: i32,
    dragging: bool,
    start_x: i32,
    start_y: i32,
    current_x: i32,
    current_y: i32,
    result: Option<SelectionResult>,
    cancelled: bool,
    capture_on_select: bool,
}

static mut SELECTION_DATA: *mut SelectionData = std::ptr::null_mut();

/// Show a fullscreen selection overlay. Blocks until selection or cancel.
/// If `capture_on_select` is true, the selected region's pixels are extracted
/// from the frozen screenshot and returned as RGBA data.
pub fn show_selection_overlay(capture_on_select: bool) -> Option<SelectionResult> {
    unsafe { show_selection_overlay_impl(capture_on_select) }
}

unsafe fn show_selection_overlay_impl(capture_on_select: bool) -> Option<SelectionResult> { unsafe {
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);

    // Capture desktop via BitBlt (HW accelerated, no format conversion)
    let desktop_dc = GetDC(None);
    let mem_dc = CreateCompatibleDC(Some(desktop_dc));
    let mem_bmp = CreateCompatibleBitmap(desktop_dc, screen_w, screen_h);
    let old_bmp = SelectObject(mem_dc, mem_bmp.into());
    let _ = BitBlt(mem_dc, 0, 0, screen_w, screen_h, Some(desktop_dc), 0, 0, SRCCOPY);
    ReleaseDC(None, desktop_dc);

    let mut data = SelectionData {
        mem_dc,
        mem_bmp,
        old_bmp,
        screen_w,
        screen_h,
        dragging: false,
        start_x: 0,
        start_y: 0,
        current_x: 0,
        current_y: 0,
        result: None,
        cancelled: false,
        capture_on_select,
    };
    SELECTION_DATA = &mut data as *mut SelectionData;

    let class_name = w!("AynimeSelectionOverlay");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(selection_wnd_proc),
        hInstance: HINSTANCE::default(),
        hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
        lpszClassName: class_name,
        ..Default::default()
    };
    RegisterClassExW(&wc);

    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class_name,
        w!("Selection"),
        WS_POPUP | WS_VISIBLE,
        0, 0, screen_w, screen_h,
        None, None, None, None,
    )
    .unwrap();

    let _ = crate::overlay::window::set_exclude_from_capture(hwnd);

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    // Cleanup
    SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(mem_bmp.into());
    let _ = DeleteDC(mem_dc);
    let _ = UnregisterClassW(class_name, None);

    SELECTION_DATA = std::ptr::null_mut();

    if data.cancelled { None } else { data.result }
} }

fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xFFFF) as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    (x, y)
}

unsafe extern "system" fn selection_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT { unsafe {
    if SELECTION_DATA.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let data = &mut *SELECTION_DATA;

    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint_overlay(hdc, data);
            EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lparam);
            data.dragging = true;
            data.start_x = x;
            data.start_y = y;
            data.current_x = x;
            data.current_y = y;
            let _ = SetCapture(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if data.dragging {
                let (x, y) = lparam_xy(lparam);
                data.current_x = x;
                data.current_y = y;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if data.dragging {
                data.dragging = false;
                let _ = ReleaseCapture();

                let (x, y) = lparam_xy(lparam);
                data.current_x = x;
                data.current_y = y;

                let left = data.start_x.min(data.current_x);
                let top = data.start_y.min(data.current_y);
                let right = data.start_x.max(data.current_x);
                let bottom = data.start_y.max(data.current_y);
                let w = right - left;
                let h = bottom - top;

                if w >= 4 && h >= 4 {
                    let rgba_data = if data.capture_on_select {
                        extract_region_rgba(data.mem_dc, left, top, w, h)
                    } else {
                        None
                    };
                    data.result = Some(SelectionResult {
                        x: left, y: top, width: w, height: h, rgba_data,
                    });
                    let _ = DestroyWindow(hwnd);
                } else {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            data.cancelled = true;
            let _ = ReleaseCapture();
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                data.cancelled = true;
                let _ = ReleaseCapture();
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
} }

unsafe fn paint_overlay(hdc: HDC, data: &SelectionData) { unsafe {
    let sw = data.screen_w;
    let sh = data.screen_h;

    // ── Double buffering: draw everything to off-screen DC, then BitBlt once ──
    let buf_dc = CreateCompatibleDC(Some(hdc));
    let buf_bmp = CreateCompatibleBitmap(hdc, sw, sh);
    let buf_old = SelectObject(buf_dc, buf_bmp.into());

    // 1. Copy frozen desktop screenshot into buffer
    let _ = BitBlt(buf_dc, 0, 0, sw, sh, Some(data.mem_dc), 0, 0, SRCCOPY);

    // 2. Semi-transparent dark overlay via AlphaBlend
    let dim_dc = CreateCompatibleDC(Some(buf_dc));
    let dim_bmp = CreateCompatibleBitmap(hdc, sw, sh);
    let dim_old = SelectObject(dim_dc, dim_bmp.into());

    let black_brush = CreateSolidBrush(COLORREF(0));
    let full_rect = RECT { left: 0, top: 0, right: sw, bottom: sh };
    FillRect(dim_dc, &full_rect, black_brush);
    let _ = DeleteObject(black_brush.into());

    let blend = BLENDFUNCTION {
        BlendOp: 0, // AC_SRC_OVER
        BlendFlags: 0,
        SourceConstantAlpha: 120,
        AlphaFormat: 0,
    };
    let _ = GdiAlphaBlend(buf_dc, 0, 0, sw, sh, dim_dc, 0, 0, sw, sh, blend);

    SelectObject(dim_dc, dim_old);
    let _ = DeleteObject(dim_bmp.into());
    let _ = DeleteDC(dim_dc);

    // 3. Draw selection or instruction text onto the buffer
    if data.dragging {
        let left = data.start_x.min(data.current_x);
        let top = data.start_y.min(data.current_y);
        let right = data.start_x.max(data.current_x);
        let bottom = data.start_y.max(data.current_y);
        let sel_w = right - left;
        let sel_h = bottom - top;

        if sel_w > 0 && sel_h > 0 {
            // Restore original screenshot in selected area (undo dim)
            let _ = BitBlt(buf_dc, left, top, sel_w, sel_h, Some(data.mem_dc), left, top, SRCCOPY);

            // Blue border
            let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00D77800));
            let old_pen = SelectObject(buf_dc, pen.into());
            let old_brush = SelectObject(buf_dc, GetStockObject(NULL_BRUSH));
            let _ = Rectangle(buf_dc, left, top, right, bottom);
            SelectObject(buf_dc, old_pen);
            SelectObject(buf_dc, old_brush);
            let _ = DeleteObject(pen.into());

            // Size label
            let text = format!("{}x{}", sel_w, sel_h);
            let text_wide: Vec<u16> = text.encode_utf16().collect();
            SetBkMode(buf_dc, TRANSPARENT);
            SetTextColor(buf_dc, COLORREF(0x00FFFFFF));
            let _ = TextOutW(buf_dc, left + 4, (top - 20).max(0), &text_wide);
        }
    } else {
        // Instruction text
        let text = "ドラッグで構え / Esc・右クリックでキャンセル";

        let font = CreateFontW(
            28, 0, 0, 0,
            FW_NORMAL.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (FF_DONTCARE.0 | DEFAULT_PITCH.0) as u32,
            w!("Yu Gothic UI"),
        );
        let old_font = SelectObject(buf_dc, font.into());

        SetBkMode(buf_dc, TRANSPARENT);
        SetTextColor(buf_dc, COLORREF(0x00FFFFFF));

        let mut text_rect = RECT {
            left: 0,
            top: sh / 2 - 20,
            right: sw,
            bottom: sh / 2 + 20,
        };
        let mut text_buf: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(buf_dc, &mut text_buf, &mut text_rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        SelectObject(buf_dc, old_font);
        let _ = DeleteObject(font.into());
    }

    // 4. Single BitBlt from buffer to screen — no flicker
    let _ = BitBlt(hdc, 0, 0, sw, sh, Some(buf_dc), 0, 0, SRCCOPY);

    // Cleanup buffer
    SelectObject(buf_dc, buf_old);
    let _ = DeleteObject(buf_bmp.into());
    let _ = DeleteDC(buf_dc);
} }

/// Extract a region from a memory DC as RGBA pixel data.
unsafe fn extract_region_rgba(src_dc: HDC, x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    unsafe {
        if w <= 0 || h <= 0 {
            return None;
        }

        // Create a DIB section to read pixel data
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };

        let tmp_dc = CreateCompatibleDC(Some(src_dc));
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(tmp_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);

        let dib = match dib {
            Ok(bmp) => bmp,
            Err(_) => {
                let _ = DeleteDC(tmp_dc);
                return None;
            }
        };

        let old = SelectObject(tmp_dc, dib.into());
        let _ = BitBlt(tmp_dc, 0, 0, w, h, Some(src_dc), x, y, SRCCOPY);

        // Read BGRA pixels and convert to RGBA
        let pixel_count = (w * h) as usize;
        let src = std::slice::from_raw_parts(bits as *const u8, pixel_count * 4);
        let mut rgba = Vec::with_capacity(pixel_count * 4);
        for pixel in src.chunks_exact(4) {
            rgba.push(pixel[2]); // R
            rgba.push(pixel[1]); // G
            rgba.push(pixel[0]); // B
            rgba.push(255);      // A
        }

        SelectObject(tmp_dc, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(tmp_dc);

        Some(rgba)
    }
}
