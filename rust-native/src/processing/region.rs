use crate::capture::screen::ScreenFrame;

/// A rectangular region on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CaptureRegion {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Clamp the region to fit within the given screen dimensions.
    pub fn clamp_to_screen(&self, screen_w: u32, screen_h: u32) -> Self {
        let x = self.x.min(screen_w.saturating_sub(1));
        let y = self.y.min(screen_h.saturating_sub(1));
        let w = self.width.min(screen_w - x);
        let h = self.height.min(screen_h - y);
        Self {
            x,
            y,
            width: w,
            height: h,
        }
    }

    /// Extract this region from a full screen frame.
    /// Returns (rgba_data, actual_width, actual_height) after clamping to screen bounds.
    pub fn extract_from_frame(&self, frame: &ScreenFrame) -> (Vec<u8>, u32, u32) {
        let region = self.clamp_to_screen(frame.width, frame.height);
        let mut out = Vec::with_capacity((region.width * region.height * 4) as usize);

        for row in 0..region.height {
            let src_y = (region.y + row) as usize;
            let src_x_start = region.x as usize;
            let src_offset = (src_y * frame.width as usize + src_x_start) * 4;
            let row_bytes = (region.width as usize) * 4;
            let src_row = &frame.data[src_offset..src_offset + row_bytes];

            // Convert BGRA -> RGBA
            for pixel in src_row.chunks_exact(4) {
                out.push(pixel[2]); // R
                out.push(pixel[1]); // G
                out.push(pixel[0]); // B
                out.push(pixel[3]); // A
            }
        }

        (out, region.width, region.height)
    }
}
