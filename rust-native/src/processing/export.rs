use anyhow::{Context, Result};
use image::{ImageBuffer, RgbaImage};
use std::path::Path;
use std::process::Command;

/// Save RGBA pixel data as a PNG file.
pub fn save_as_png(
    data: &[u8],
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    let img: RgbaImage = ImageBuffer::from_raw(width, height, data.to_vec())
        .context("Failed to create image buffer from raw data")?;
    img.save(path)
        .with_context(|| format!("Failed to save PNG to {}", path.display()))?;
    log::info!("Saved PNG: {} ({}x{})", path.display(), width, height);
    Ok(())
}

/// Save RGBA pixel data as a WebP file.
pub fn save_as_webp(
    data: &[u8],
    width: u32,
    height: u32,
    path: &Path,
    quality: u8,
) -> Result<()> {
    let img: RgbaImage = ImageBuffer::from_raw(width, height, data.to_vec())
        .context("Failed to create image buffer from raw data")?;
    // image crate supports WebP encoding
    img.save(path)
        .with_context(|| format!("Failed to save WebP to {}", path.display()))?;
    log::info!("Saved WebP: {} ({}x{}, q={})", path.display(), width, height, quality);
    Ok(())
}

/// Encode a series of RGBA frames into an MP4 using FFmpeg.
/// `ffmpeg_path` - path to the ffmpeg binary
/// `frames` - list of (rgba_data, width, height) tuples
/// `fps` - frames per second
/// `output_path` - output .mp4 path
/// `max_size_bytes` - optional file size limit; will re-encode at lower quality if exceeded
pub fn encode_mp4(
    ffmpeg_path: &Path,
    frames: &[(Vec<u8>, u32, u32)],
    fps: u32,
    output_path: &Path,
    max_size_bytes: Option<u64>,
) -> Result<()> {
    if frames.is_empty() {
        anyhow::bail!("No frames to encode");
    }

    let (_, width, height) = &frames[0];
    let width = *width;
    let height = *height;

    if width < 2 || height < 2 {
        anyhow::bail!(
            "Frame dimensions too small for encoding: {}x{} (minimum 2x2)",
            width,
            height
        );
    }

    log::info!(
        "Encoding MP4: {}x{}, {} frames @ {} fps",
        width,
        height,
        frames.len(),
        fps
    );

    // Write raw frames to a temp file for ffmpeg piped input
    let temp_dir = output_path.parent().unwrap_or(Path::new("."));
    let raw_path = temp_dir.join("_temp_frames.raw");

    // Concatenate all RGBA frames
    let mut raw_data = Vec::new();
    for (data, _, _) in frames {
        raw_data.extend_from_slice(data);
    }
    std::fs::write(&raw_path, &raw_data)
        .context("Failed to write temp raw frames")?;

    // First pass: encode at CRF 23
    let result = encode_mp4_with_crf(ffmpeg_path, &raw_path, width, height, fps, output_path, 23);
    let _ = std::fs::remove_file(&raw_path);
    result?;

    // Check file size and re-encode if needed
    if let Some(max_size) = max_size_bytes {
        let file_size = std::fs::metadata(output_path)
            .context("Failed to read output file metadata")?
            .len();

        if file_size > max_size {
            log::info!(
                "MP4 size {} exceeds limit {}, re-encoding with lower quality",
                file_size,
                max_size
            );

            // Re-write raw frames
            std::fs::write(&raw_path, &raw_data)
                .context("Failed to write temp raw frames for re-encode")?;

            // Progressively increase CRF until under limit
            for crf in [28, 33, 38, 43, 48] {
                let _ = std::fs::remove_file(output_path);
                encode_mp4_with_crf(
                    ffmpeg_path, &raw_path, width, height, fps, output_path, crf,
                )?;

                let new_size = std::fs::metadata(output_path)
                    .context("Failed to read re-encoded file metadata")?
                    .len();

                if new_size <= max_size {
                    log::info!("Re-encoded MP4 at CRF {} -> {} bytes", crf, new_size);
                    break;
                }
            }
            let _ = std::fs::remove_file(&raw_path);
        }
    }

    Ok(())
}

fn encode_mp4_with_crf(
    ffmpeg_path: &Path,
    raw_input: &Path,
    width: u32,
    height: u32,
    fps: u32,
    output_path: &Path,
    crf: u32,
) -> Result<()> {
    // Scale to even dimensions (libx264 requires width/height divisible by 2)
    let vf = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

    let output = Command::new(ffmpeg_path)
        .args([
            "-y",
            "-f", "rawvideo",
            "-pix_fmt", "rgba",
            "-s", &format!("{}x{}", width, height),
            "-r", &fps.to_string(),
            "-i", &raw_input.to_string_lossy(),
            "-vf", vf,
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-crf", &crf.to_string(),
            "-preset", "medium",
            &output_path.to_string_lossy(),
        ])
        .output()
        .context("Failed to run ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("FFmpeg failed (CRF {}): {}", crf, stderr);
    }

    Ok(())
}

/// Encode frames as GIF using FFmpeg.
pub fn encode_gif(
    ffmpeg_path: &Path,
    frames: &[(Vec<u8>, u32, u32)],
    fps: u32,
    output_path: &Path,
    max_size_bytes: Option<u64>,
) -> Result<()> {
    if frames.is_empty() {
        anyhow::bail!("No frames to encode");
    }

    let (_, width, height) = &frames[0];
    let width = *width;
    let height = *height;

    if width < 2 || height < 2 {
        anyhow::bail!(
            "Frame dimensions too small for encoding: {}x{} (minimum 2x2)",
            width,
            height
        );
    }

    log::info!(
        "Encoding GIF: {}x{}, {} frames @ {} fps",
        width,
        height,
        frames.len(),
        fps
    );

    let temp_dir = output_path.parent().unwrap_or(Path::new("."));
    let raw_path = temp_dir.join("_temp_frames_gif.raw");
    let palette_path = temp_dir.join("_temp_palette.png");

    let mut raw_data = Vec::new();
    for (data, _, _) in frames {
        raw_data.extend_from_slice(data);
    }
    std::fs::write(&raw_path, &raw_data)
        .context("Failed to write temp raw frames")?;

    let raw_input_args = [
        "-f", "rawvideo",
        "-pix_fmt", "rgba",
        "-s", &format!("{}x{}", width, height),
        "-r", &fps.to_string(),
        "-i", &raw_path.to_string_lossy(),
    ];

    // Generate palette
    let output = Command::new(ffmpeg_path)
        .args(&raw_input_args)
        .args([
            "-vf", "palettegen=stats_mode=diff",
            "-y",
            &palette_path.to_string_lossy(),
        ])
        .output()
        .context("Failed to generate GIF palette")?;

    if !output.status.success() {
        let _ = std::fs::remove_file(&raw_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("FFmpeg palette generation failed: {}", stderr);
    }

    // Encode GIF with palette
    // For size control, scale down if needed
    let mut scale_factor = 1.0_f32;
    loop {
        let scaled_w = ((width as f32) * scale_factor) as u32 & !1; // even number
        let scaled_h = ((height as f32) * scale_factor) as u32 & !1;

        let filter = if scale_factor < 1.0 {
            format!(
                "scale={}:{}:flags=lanczos [scaled]; [scaled][1:v] paletteuse=dither=bayer",
                scaled_w, scaled_h
            )
        } else {
            "[0:v][1:v] paletteuse=dither=bayer".to_string()
        };

        let _ = std::fs::remove_file(output_path);
        let result = Command::new(ffmpeg_path)
            .args(&raw_input_args)
            .args(["-i", &palette_path.to_string_lossy()])
            .args(["-lavfi", &filter])
            .args(["-y", &output_path.to_string_lossy()])
            .output()
            .context("Failed to encode GIF")?;

        if !result.status.success() {
            let _ = std::fs::remove_file(&raw_path);
            let _ = std::fs::remove_file(&palette_path);
            let stderr = String::from_utf8_lossy(&result.stderr);
            anyhow::bail!("FFmpeg GIF encoding failed: {}", stderr);
        }

        if let Some(max_size) = max_size_bytes {
            let file_size = std::fs::metadata(output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            if file_size > max_size && scale_factor > 0.3 {
                scale_factor -= 0.1;
                log::info!(
                    "GIF size {} exceeds limit {}, retrying at scale {:.1}",
                    file_size,
                    max_size,
                    scale_factor
                );
                continue;
            }
        }
        break;
    }

    let _ = std::fs::remove_file(&raw_path);
    let _ = std::fs::remove_file(&palette_path);

    Ok(())
}
