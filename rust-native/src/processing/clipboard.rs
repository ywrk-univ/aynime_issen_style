//! Copy RGBA image data to the Windows clipboard as a DIB (Device Independent Bitmap).

use anyhow::{Context, Result};
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_DIB;

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
