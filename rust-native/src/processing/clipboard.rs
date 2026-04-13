//! Copy RGBA image data to the Windows clipboard as a DIB (Device Independent Bitmap).

use anyhow::{Context, Result};
use std::path::Path;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP};

/// Copy RGBA pixel data to the clipboard as a bitmap.
/// The clipboard will hold a DIB that can be pasted into Discord, Paint, etc.
pub fn copy_rgba_to_clipboard(data: &[u8], width: u32, height: u32) -> Result<()> {
    unsafe { copy_rgba_to_clipboard_impl(data, width, height) }
}

unsafe fn copy_rgba_to_clipboard_impl(data: &[u8], width: u32, height: u32) -> Result<()> {
    unsafe {
        // BITMAPINFOHEADER (40 bytes) + pixel data (BGRA, bottom-up)
        let header_size = 40u32;
        let row_bytes = (width * 3 + 3) & !3; // 24-bit rows aligned to 4 bytes
        let pixel_size = row_bytes * height;
        let total_size = header_size as usize + pixel_size as usize;

        // Allocate global memory for clipboard
        let hmem = GlobalAlloc(GMEM_MOVEABLE, total_size)
            .context("Failed to allocate global memory")?;
        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(hmem));
            anyhow::bail!("GlobalLock returned null");
        }

        // Write BITMAPINFOHEADER
        let header = ptr as *mut [u8; 40];
        (*header) = [0u8; 40];
        // biSize = 40
        *(ptr.add(0) as *mut u32) = 40;
        // biWidth
        *(ptr.add(4) as *mut i32) = width as i32;
        // biHeight (positive = bottom-up)
        *(ptr.add(8) as *mut i32) = height as i32;
        // biPlanes = 1
        *(ptr.add(12) as *mut u16) = 1;
        // biBitCount = 24
        *(ptr.add(14) as *mut u16) = 24;
        // biCompression = BI_RGB = 0
        *(ptr.add(16) as *mut u32) = 0;
        // biSizeImage
        *(ptr.add(20) as *mut u32) = pixel_size;

        // Write pixel data (RGBA → BGR, flip vertically for bottom-up DIB)
        let pixels = ptr.add(header_size as usize);
        for y in 0..height {
            let src_row = (height - 1 - y) as usize; // flip
            let dst_offset = (y * row_bytes) as usize;
            for x in 0..width {
                let src_idx = (src_row * width as usize + x as usize) * 4;
                let dst_idx = dst_offset + (x as usize) * 3;
                // RGBA → BGR
                *pixels.add(dst_idx) = data[src_idx + 2];     // B
                *pixels.add(dst_idx + 1) = data[src_idx + 1]; // G
                *pixels.add(dst_idx + 2) = data[src_idx];     // R
            }
        }

        let _ = GlobalUnlock(hmem);

        // Set clipboard
        OpenClipboard(None).context("Failed to open clipboard")?;
        let _ = EmptyClipboard();
        SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(hmem.0)))
            .context("Failed to set clipboard data")?;
        let _ = CloseClipboard();

        log::info!("Copied {}x{} image to clipboard", width, height);
        Ok(())
    }
}

/// Copy a file to the clipboard as CF_HDROP (file drop).
/// This allows pasting the file into Discord, Explorer, etc.
pub fn copy_file_to_clipboard(path: &Path) -> Result<()> {
    unsafe { copy_file_to_clipboard_impl(path) }
}

unsafe fn copy_file_to_clipboard_impl(path: &Path) -> Result<()> {
    unsafe {
        let abs_path = std::fs::canonicalize(path)
            .with_context(|| format!("Failed to canonicalize {}", path.display()))?;
        let path_wide: Vec<u16> = abs_path
            .to_string_lossy()
            // canonicalize returns \\?\ prefix on Windows, strip it
            .trim_start_matches("\\\\?\\")
            .encode_utf16()
            .chain(std::iter::once(0)) // null terminator for the path
            .chain(std::iter::once(0)) // double null terminator for the list
            .collect();

        // DROPFILES header (20 bytes) + wide string data
        let header_size = 20usize; // sizeof(DROPFILES)
        let data_size = path_wide.len() * 2;
        let total_size = header_size + data_size;

        let hmem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total_size)
            .context("Failed to allocate global memory for HDROP")?;
        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(hmem));
            anyhow::bail!("GlobalLock returned null");
        }

        // DROPFILES struct:
        //   DWORD pFiles (offset to file list) = 20
        //   POINT pt = {0, 0}
        //   BOOL fNC = FALSE
        //   BOOL fWide = TRUE
        *(ptr as *mut u32) = header_size as u32; // pFiles
        // pt.x = 0, pt.y = 0 (already zeroed)
        // fNC = 0 (already zeroed)
        *(ptr.add(16) as *mut u32) = 1; // fWide = TRUE

        // Copy wide string file path after header
        std::ptr::copy_nonoverlapping(
            path_wide.as_ptr() as *const u8,
            ptr.add(header_size),
            data_size,
        );

        let _ = GlobalUnlock(hmem);

        OpenClipboard(None).context("Failed to open clipboard")?;
        let _ = EmptyClipboard();
        SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hmem.0)))
            .context("Failed to set clipboard data (HDROP)")?;
        let _ = CloseClipboard();

        log::info!("Copied file to clipboard: {}", abs_path.display());
        Ok(())
    }
}
