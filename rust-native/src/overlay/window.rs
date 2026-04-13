use anyhow::{Context, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, LoadCursorW, SetCursor, SetWindowDisplayAffinity, IDC_ARROW,
    WINDOW_DISPLAY_AFFINITY,
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
    set_exclude_from_capture(hwnd)?;
    Ok(hwnd)
}
