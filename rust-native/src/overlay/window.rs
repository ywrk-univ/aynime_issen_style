use anyhow::{Context, Result};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowLongW, LoadCursorW, SetCursor, SetLayeredWindowAttributes,
    SetWindowDisplayAffinity, SetWindowLongW, GWL_EXSTYLE, IDC_ARROW, LAYERED_WINDOW_ATTRIBUTES_FLAGS,
    WINDOW_DISPLAY_AFFINITY, WS_EX_LAYERED,
};

/// WDA_EXCLUDEFROMCAPTURE = 0x00000011 (Windows 10 2004+)
/// Makes the window invisible to screen capture APIs.
const WDA_EXCLUDEFROMCAPTURE: WINDOW_DISPLAY_AFFINITY = WINDOW_DISPLAY_AFFINITY(0x00000011);

/// Apply WDA_EXCLUDEFROMCAPTURE to the given HWND.
pub fn set_exclude_from_capture(hwnd: HWND) -> Result<()> {
    unsafe {
        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
            .context("Failed to set WDA_EXCLUDEFROMCAPTURE. Requires Windows 10 2004+")?;
    }
    log::info!("WDA_EXCLUDEFROMCAPTURE set on HWND {:?}", hwnd.0);
    Ok(())
}

/// Hide the system cursor. Call `show_cursor()` to restore.
pub fn hide_cursor() {
    unsafe {
        let _ = SetCursor(None);
    }
}

/// Restore the system cursor to the default arrow.
pub fn show_cursor() {
    unsafe {
        if let Ok(cursor) = LoadCursorW(None, IDC_ARROW) {
            let _ = SetCursor(Some(cursor));
        }
    }
}

/// Find our eframe window by title and apply the capture exclusion.
pub fn apply_capture_exclusion_to_egui_window(title: &str) -> Result<HWND> {
    let hwnd = find_window_by_title(title)?;
    set_exclude_from_capture(hwnd)?;
    Ok(hwnd)
}

/// Find a window by its title text.
pub fn find_window_by_title(title: &str) -> Result<HWND> {
    let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let hwnd =
        unsafe { FindWindowW(None, windows::core::PCWSTR(title_wide.as_ptr())) }
            .context("Could not find window")?;
    if hwnd.0.is_null() {
        anyhow::bail!(
            "Could not find window with title '{}'. Is the window created?",
            title
        );
    }
    Ok(hwnd)
}

/// Set window opacity via WS_EX_LAYERED + SetLayeredWindowAttributes.
/// `alpha`: 0 = fully transparent, 255 = fully opaque.
pub fn set_window_opacity(hwnd: HWND, alpha: u8) -> Result<()> {
    unsafe {
        // Add WS_EX_LAYERED if not already set
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if ex_style & WS_EX_LAYERED.0 as i32 == 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
        }
        // LWA_ALPHA = 0x02
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0x02))
            .context("SetLayeredWindowAttributes failed")?;
        let _ = UpdateWindow(hwnd);
    }
    log::debug!("Window opacity set to {}/255", alpha);
    Ok(())
}

/// Remove the WS_EX_LAYERED style to restore full opacity.
pub fn clear_window_opacity(hwnd: HWND) -> Result<()> {
    unsafe {
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if ex_style & WS_EX_LAYERED.0 as i32 != 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style & !(WS_EX_LAYERED.0 as i32));
        }
        let _ = UpdateWindow(hwnd);
    }
    Ok(())
}
