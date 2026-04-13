use anyhow::{Context, Result};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateCursor, CopyIcon, FindWindowW, GetWindowLongW, SetLayeredWindowAttributes,
    SetSystemCursor, SetWindowDisplayAffinity, SetWindowLongW, SystemParametersInfoW,
    GWL_EXSTYLE, LAYERED_WINDOW_ATTRIBUTES_FLAGS,
    SPI_SETCURSORS, SYSTEM_CURSOR_ID, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
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

/// 全カーソル種別の ID。SetSystemCursor で透明カーソルに置換する対象。
const ALL_CURSOR_IDS: &[SYSTEM_CURSOR_ID] = &[
    SYSTEM_CURSOR_ID(32512), // OCR_NORMAL
    SYSTEM_CURSOR_ID(32513), // OCR_IBEAM
    SYSTEM_CURSOR_ID(32514), // OCR_WAIT
    SYSTEM_CURSOR_ID(32515), // OCR_CROSS
    SYSTEM_CURSOR_ID(32516), // OCR_UP
    SYSTEM_CURSOR_ID(32642), // OCR_SIZENWSE
    SYSTEM_CURSOR_ID(32643), // OCR_SIZENESW
    SYSTEM_CURSOR_ID(32644), // OCR_SIZEWE
    SYSTEM_CURSOR_ID(32645), // OCR_SIZENS
    SYSTEM_CURSOR_ID(32646), // OCR_SIZEALL
    SYSTEM_CURSOR_ID(32648), // OCR_NO
    SYSTEM_CURSOR_ID(32649), // OCR_HAND
    SYSTEM_CURSOR_ID(32650), // OCR_APPSTARTING
];

/// システム全体のカーソルを透明に置換する。
/// SetSystemCursor はシステム全体に効くため、カスタムカーソルソフトの
/// オーバーレイにも対応できる。`show_cursor()` で復元すること。
pub fn hide_cursor() {
    unsafe {
        // 1x1 の完全透明カーソルを作成
        // AND mask: 0xFF = 全ビット透明, XOR mask: 0x00 = 描画なし
        let and_plane: [u8; 4] = [0xFF; 4];
        let xor_plane: [u8; 4] = [0x00; 4];
        let blank = CreateCursor(
            None,
            0,
            0,
            1,
            1,
            and_plane.as_ptr() as *const _,
            xor_plane.as_ptr() as *const _,
        );
        let Ok(blank) = blank else {
            log::warn!("Failed to create blank cursor");
            return;
        };

        // 各カーソル種別を透明カーソルに置換
        // SetSystemCursor はハンドルの所有権を奪うため、毎回コピーが必要
        for &id in ALL_CURSOR_IDS {
            if let Ok(copy) = CopyIcon(std::mem::transmute(blank)) {
                let _ = SetSystemCursor(std::mem::transmute(copy), id);
            }
        }
        // 元の blank は CopyIcon のソースとして使ったが所有権は残っている
        // Windows がカーソルリソースを管理するため明示的な破棄は不要
    }
}

/// システムカーソルをレジストリのデフォルト（ユーザー設定含む）に復元する。
pub fn show_cursor() {
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_SETCURSORS,
            0,
            None,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
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
