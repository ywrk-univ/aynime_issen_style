//! Extract anime title from browser window titles.
//!
//! Ported from the original Python implementation in
//! `src/utils/capture/target.py::get_nime_window_text`.

use windows::core as windows_core;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Result of anime title extraction.
pub struct AnimeTitleResult {
    /// The extracted/cleaned title.
    pub title: String,
    /// Whether this looks like an anime streaming page.
    pub is_anime: bool,
}

/// Get all visible window titles and try to find an anime title.
/// Returns the best match (anime streaming page preferred).
pub fn find_anime_title() -> Option<AnimeTitleResult> {
    let windows = enumerate_window_titles();

    // First pass: look for anime streaming pages
    for (_, title) in &windows {
        let result = extract_anime_title(title);
        if result.is_anime {
            return Some(result);
        }
    }

    None
}

/// Extract anime title from a window title string.
/// Returns (cleaned_title, is_anime).
pub fn extract_anime_title(text: &str) -> AnimeTitleResult {
    let text = text.trim();
    if text.is_empty() {
        return AnimeTitleResult {
            title: String::new(),
            is_anime: false,
        };
    }

    // Strip browser suffix to get the page title
    let (text, is_browser) = if text.ends_with("Mozilla Firefox") {
        (text.replace(" - Mozilla Firefox", ""), true)
    } else if text.ends_with("Google Chrome") {
        (text.replace(" - Google Chrome", ""), true)
    } else if text.ends_with("Microsoft Edge") {
        (text.replace(" - Microsoft Edge", ""), true)
    } else if text.ends_with(" - Discord") {
        // Discord stream shows channel name, not anime
        let t = text.replace(" - Discord", "");
        return AnimeTitleResult {
            title: t,
            is_anime: false,
        };
    } else {
        return AnimeTitleResult {
            title: text.to_string(),
            is_anime: false,
        };
    };

    if !is_browser {
        return AnimeTitleResult {
            title: text,
            is_anime: false,
        };
    }

    // Streaming service-specific extraction
    let (title, is_anime) = if text.ends_with("dアニメストア") {
        if text.contains("アニメ動画見放題") {
            // Series page (no episode info) — not useful
            (text, false)
        } else {
            // Format: "<anime> - <episode> - <episode_title> dアニメストア"
            // Keep only anime + episode
            let t = text.replace(" dアニメストア", "");
            let parts: Vec<&str> = t.split(" - ").collect();
            let t = if parts.len() >= 2 {
                format!("{} {}", parts[0], parts[1])
            } else {
                parts[0].to_string()
            };
            (t, true)
        }
    } else if text.ends_with("AnimeFesta") {
        let t = text.replace("を見る AnimeFesta", "");
        (t, true)
    } else if let Some(bc_pos) = text.find("バンダイチャンネル") {
        let mut t = text[..bc_pos].to_string();
        if t.ends_with("- ") {
            t = t[..t.len() - 2].to_string();
        }
        (t, true)
    } else if text.ends_with("Prime Video") {
        let t = text
            .replace("Amazon.co.jp ", "")
            .replace("を観る Prime Video", "");
        (t, true)
    } else if text.ends_with("ABEMA") {
        let t = text
            .replace("(アニメ)", "")
            .replace("無料動画・見逃し配信を見るなら", "")
            .replace("ABEMA", "");
        (t, true)
    } else if text.ends_with("ニコニコ") || text.contains("nicovideo") {
        let t = text
            .replace(" - ニコニコ", "")
            .replace(" - ニコニコ動画", "");
        (t, true)
    } else if text.contains("Crunchyroll") {
        let t = text.replace(" - Crunchyroll", "");
        (t, true)
    } else {
        (text, false)
    };

    // Clean up whitespace
    let title = title.trim().to_string();
    let title = collapse_spaces(&title);

    AnimeTitleResult { title, is_anime }
}

/// Sanitize a title for use in filenames.
/// Removes characters that are invalid in Windows filenames.
pub fn sanitize_for_filename(title: &str) -> String {
    let mut s = String::with_capacity(title.len());
    for c in title.chars() {
        match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => s.push('_'),
            '\0'..='\x1f' => {} // skip control chars
            _ => s.push(c),
        }
    }
    // Trim trailing dots/spaces (Windows doesn't allow them at end of filename)
    let s = s.trim_end_matches(|c| c == '.' || c == ' ').to_string();
    if s.is_empty() {
        "unknown".to_string()
    } else {
        s
    }
}

/// Enumerate all visible window titles. Returns (HWND, title) pairs.
fn enumerate_window_titles() -> Vec<(isize, String)> {
    let mut results = Vec::new();

    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            windows::Win32::Foundation::LPARAM(&mut results as *mut Vec<(isize, String)> as isize),
        );
    }

    results
}

unsafe extern "system" fn enum_windows_proc(
    hwnd: HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows_core::BOOL { unsafe {
    let results = &mut *(lparam.0 as *mut Vec<(isize, String)>);

    if !IsWindowVisible(hwnd).as_bool() {
        return windows_core::BOOL(1);
    }

    if IsIconic(hwnd).as_bool() {
        return windows_core::BOOL(1);
    }

    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return windows_core::BOOL(1);
    }

    let mut buf = vec![0u16; (len + 1) as usize];
    let actual = GetWindowTextW(hwnd, &mut buf);
    if actual > 0 {
        let title = String::from_utf16_lossy(&buf[..actual as usize]);
        if !title.is_empty() && title != "Program Manager" {
            results.push((hwnd.0 as isize, title));
        }
    }

    windows_core::BOOL(1)
} }

fn collapse_spaces(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c == ' ' {
            if !prev_space {
                result.push(c);
            }
            prev_space = true;
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    result
}
