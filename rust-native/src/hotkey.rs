//! Global hotkey registration via Win32 RegisterHotKey.
//!
//! Spawns a dedicated thread with a Win32 message pump that listens for
//! system-wide hotkeys and communicates events back through an `Arc<Mutex<Vec>>`.

use std::sync::{Arc, Mutex};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

// Virtual-key codes
const VK_Z: u32 = 0x5A;
const VK_X: u32 = 0x58;

// Hotkey IDs
const HOTKEY_STILL_CAPTURE: i32 = 1;
const HOTKEY_RECORD_TOGGLE: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    StillCapture,
    RecordToggle,
}

/// Shared queue for hotkey events.
pub type HotkeyReceiver = Arc<Mutex<Vec<HotkeyAction>>>;

/// Spawn the hotkey listener thread.  Returns the shared event queue.
pub fn start_hotkey_listener() -> HotkeyReceiver {
    let queue: HotkeyReceiver = Arc::new(Mutex::new(Vec::new()));
    let tx = queue.clone();

    std::thread::spawn(move || unsafe {
        let modifiers: HOT_KEY_MODIFIERS = MOD_CONTROL | MOD_SHIFT;

        if let Err(e) = RegisterHotKey(None, HOTKEY_STILL_CAPTURE, modifiers, VK_Z) {
            log::error!("Failed to register Ctrl+Shift+Z hotkey: {}", e);
        } else {
            log::info!("Global hotkey registered: Ctrl+Shift+Z → still capture");
        }

        if let Err(e) = RegisterHotKey(None, HOTKEY_RECORD_TOGGLE, modifiers, VK_X) {
            log::error!("Failed to register Ctrl+Shift+X hotkey: {}", e);
        } else {
            log::info!("Global hotkey registered: Ctrl+Shift+X → record toggle");
        }

        // Message pump — blocks until WM_QUIT or error
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0 as i32;
                let action = match id {
                    HOTKEY_STILL_CAPTURE => Some(HotkeyAction::StillCapture),
                    HOTKEY_RECORD_TOGGLE => Some(HotkeyAction::RecordToggle),
                    _ => None,
                };
                if let Some(action) = action {
                    log::info!("Hotkey fired: {:?}", action);
                    tx.lock().unwrap().push(action);
                }
            }
        }

        // Cleanup (reached on WM_QUIT)
        let _ = UnregisterHotKey(None, HOTKEY_STILL_CAPTURE);
        let _ = UnregisterHotKey(None, HOTKEY_RECORD_TOGGLE);
    });

    queue
}
