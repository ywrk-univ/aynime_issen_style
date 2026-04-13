use anyhow::{Context, Result};
use image::{ImageBuffer, RgbaImage};
use std::path::Path;
use std::process::Command;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::config::GifQualityMode;

/// Windows: CREATE_NO_WINDOW (0x08000000) を付与してコンソールウィンドウを非表示にする
#[cfg(target_os = "windows")]
fn hide_console_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_console_window(_cmd: &mut Command) {}

/// ffmpeg で利用可能な H.264 エンコーダのキャッシュ
static H264_ENCODERS: OnceLock<HashSet<String>> = OnceLock::new();

/// ffmpeg に問い合わせて利用可能なビデオエンコーダ一覧を取得する
fn enumerate_h264_encoders(ffmpeg_path: &Path) -> &'static HashSet<String> {
    H264_ENCODERS.get_or_init(|| {
        let mut cmd = Command::new(ffmpeg_path);
        cmd.args(["-hide_banner", "-encoders"]);
        hide_console_window(&mut cmd);
        let output = cmd.output();
        let mut encoders = HashSet::new();
        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout);
            // ffmpeg -encoders の出力形式: " V..... h264_nvenc ..."
            let re = regex::Regex::new(r"([VAS\.])[F\.][S\.][X\.][B\.][D\.]\s+(\w+)").unwrap();
            for cap in re.captures_iter(&text) {
                if &cap[1] == "V" {
                    encoders.insert(cap[2].to_string());
                }
            }
        }
        log::info!("Available H.264 encoders: {:?}", encoders);
        encoders
    })
}

/// CPU 使用量を抑制するための共通引数を追加するヘルパー。
/// ffmpeg の -threads オプションと、Windows の BELOW_NORMAL プロセス優先度を設定する。
fn cpu_throttled_command(ffmpeg_path: &Path) -> Command {
    let mut cmd = Command::new(ffmpeg_path);
    // 論理コア数の半分に制限（最低 1）
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        / 2;
    let threads = threads.max(1);
    cmd.args(["-threads", &threads.to_string()]);
    // Windows: BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW);
    }
    cmd
}

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

/// Save RGBA pixel data as a JPEG file.
pub fn save_as_jpeg(
    data: &[u8],
    width: u32,
    height: u32,
    path: &Path,
    quality: u8,
) -> Result<()> {
    let img: RgbaImage = ImageBuffer::from_raw(width, height, data.to_vec())
        .context("Failed to create image buffer from raw data")?;
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality);
    image::ImageEncoder::write_image(
        encoder,
        &rgb,
        width,
        height,
        image::ColorType::Rgb8.into(),
    )
    .with_context(|| format!("Failed to encode JPEG to {}", path.display()))?;
    log::info!("Saved JPEG: {} ({}x{}, q={})", path.display(), width, height, quality);
    Ok(())
}

/// Save RGBA pixel data as a BMP file.
pub fn save_as_bmp(
    data: &[u8],
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    let img: RgbaImage = ImageBuffer::from_raw(width, height, data.to_vec())
        .context("Failed to create image buffer from raw data")?;
    img.save(path)
        .with_context(|| format!("Failed to save BMP to {}", path.display()))?;
    log::info!("Saved BMP: {} ({}x{})", path.display(), width, height);
    Ok(())
}

/// Save RGBA pixel data in the given format (by extension).
pub fn save_still(
    data: &[u8],
    width: u32,
    height: u32,
    path: &Path,
    format: &str,
) -> Result<()> {
    match format.to_uppercase().as_str() {
        "PNG" => save_as_png(data, width, height, path),
        "WEBP" => save_as_webp(data, width, height, path, 85),
        "JPEG" | "JPG" => save_as_jpeg(data, width, height, path, 90),
        "BMP" => save_as_bmp(data, width, height, path),
        _ => anyhow::bail!("Unsupported still format: {}", format),
    }
}

/// Get file extension for a format name.
pub fn format_extension(format: &str) -> &str {
    match format.to_uppercase().as_str() {
        "PNG" => "png",
        "WEBP" => "webp",
        "JPEG" | "JPG" => "jpg",
        "BMP" => "bmp",
        "GIF" => "gif",
        "MP4" => "mp4",
        "WEBM" => "webm",
        _ => "bin",
    }
}

/// H.264 エンコーダの設定候補。
/// Python 側 (video_encoder.py) と同じ方針:
///   - HW エンコーダを優先 (NVENC > AMF > QSV)
///   - リッチ設定 → 最小設定 の順にフォールバック
///   - 低速エンコード・高画質・可変ビットレート
struct H264EncoderArgs {
    label: &'static str,
    args: Vec<&'static str>,
}

/// 試行するエンコーダ設定リストを構築する
fn build_h264_encoder_candidates(encoders: &HashSet<String>) -> Vec<H264EncoderArgs> {
    let mut detailed: Vec<H264EncoderArgs> = Vec::new();
    let mut compat: Vec<H264EncoderArgs> = Vec::new();

    if encoders.contains("h264_nvenc") {
        detailed.push(H264EncoderArgs {
            label: "h264_nvenc (rich)",
            args: vec![
                "-c:v", "h264_nvenc",
                "-preset", "p7",
                "-profile", "main",
                "-multipass", "fullres",
                "-rc", "vbr_hq",
                "-cq", "19",
                "-b:v", "0",
                "-rc-lookahead", "20",
                "-spatial-aq", "1",
                "-aq-strength", "10",
                "-temporal-aq", "1",
                "-bf", "4",
            ],
        });
        compat.push(H264EncoderArgs {
            label: "h264_nvenc (compat)",
            args: vec!["-c:v", "h264_nvenc"],
        });
    }
    if encoders.contains("h264_amf") {
        detailed.push(H264EncoderArgs {
            label: "h264_amf (rich)",
            args: vec![
                "-c:v", "h264_amf",
                "-usage", "high_quality",
                "-profile", "main",
                "-quality", "quality",
                "-preset", "quality",
            ],
        });
        compat.push(H264EncoderArgs {
            label: "h264_amf (compat)",
            args: vec!["-c:v", "h264_amf"],
        });
    }
    if encoders.contains("h264_qsv") {
        detailed.push(H264EncoderArgs {
            label: "h264_qsv (rich)",
            args: vec![
                "-c:v", "h264_qsv",
                "-preset", "veryslow",
                "-global_quality", "19",
            ],
        });
        compat.push(H264EncoderArgs {
            label: "h264_qsv (compat)",
            args: vec!["-c:v", "h264_qsv"],
        });
    }

    // リッチ設定を全部試してから、最小設定を試す (Python 側と同じ順序)
    detailed.extend(compat);
    detailed
}

/// Encode a series of RGBA frames into an MP4 using FFmpeg.
/// Python 側 (video_encoder.py) と同じ方針で HW エンコーダを使用する。
/// `ffmpeg_path` - path to the ffmpeg binary
/// `frames` - list of (rgba_data, width, height) tuples
/// `fps` - frames per second
/// `output_path` - output .mp4 path
/// `max_size_bytes` - optional file size limit (HW エンコーダではサイズ制御が難しいため best-effort)
pub fn encode_mp4(
    ffmpeg_path: &Path,
    frames: &[(Vec<u8>, u32, u32)],
    fps: u32,
    output_path: &Path,
    _max_size_bytes: Option<u64>,
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

    // HW エンコーダ候補を構築
    let encoders = enumerate_h264_encoders(ffmpeg_path);
    let candidates = build_h264_encoder_candidates(encoders);
    if candidates.is_empty() {
        anyhow::bail!("No H.264 HW encoder available (h264_nvenc / h264_amf / h264_qsv)");
    }

    // raw フレームを一時ファイルに書き出す
    let temp_dir = output_path.parent().unwrap_or(Path::new("."));
    let raw_path = temp_dir.join("_temp_frames.raw");
    let mut raw_data = Vec::new();
    for (data, _, _) in frames {
        raw_data.extend_from_slice(data);
    }
    std::fs::write(&raw_path, &raw_data)
        .context("Failed to write temp raw frames")?;

    // 候補を順に試行
    let mut last_error = None;
    for candidate in &candidates {
        let _ = std::fs::remove_file(output_path);
        let mut cmd = Command::new(ffmpeg_path);
        hide_console_window(&mut cmd);
        cmd.args([
            "-y",
            "-hide_banner",
            "-loglevel", "error",
            "-f", "rawvideo",
            "-pix_fmt", "rgba",
            "-s", &format!("{}x{}", width, height),
            "-r", &fps.to_string(),
            "-i", &raw_path.to_string_lossy(),
            "-an", "-sn", "-dn",
            "-pix_fmt", "yuv420p",
            "-movflags", "+faststart",
        ]);
        for arg in &candidate.args {
            cmd.arg(arg);
        }
        cmd.arg(output_path);

        match cmd.output() {
            Ok(output) if output.status.success() => {
                log::info!("MP4 encode succeeded with {}", candidate.label);
                let _ = std::fs::remove_file(&raw_path);
                return Ok(());
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::warn!(
                    "MP4 encode failed with {}: {}",
                    candidate.label,
                    stderr
                );
                last_error = Some(format!("{}: {}", candidate.label, stderr));
            }
            Err(e) => {
                log::warn!("Failed to run ffmpeg with {}: {}", candidate.label, e);
                last_error = Some(format!("{}: {}", candidate.label, e));
            }
        }
    }

    let _ = std::fs::remove_file(&raw_path);
    anyhow::bail!(
        "MP4 encode failed with all HW encoder candidates. Last error: {}",
        last_error.unwrap_or_default()
    );
}

/// Encode frames as GIF using gifski.
/// ffmpeg を経由せず、メモリ上の RGBA フレームから直接 GIF を生成する。
///   - プロセス起動ゼロ、ディスクI/Oゼロ
///   - Collector (フレーム投入) と Writer (エンコード) がマルチスレッドで並列動作
///   - fast モードで高速量子化
pub fn encode_gif(
    _ffmpeg_path: &Path,
    frames: &[(Vec<u8>, u32, u32)],
    fps: u32,
    output_path: &Path,
    _max_size_bytes: Option<u64>,
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
        "Encoding GIF (gifski): {}x{}, {} frames @ {} fps",
        width,
        height,
        frames.len(),
        fps
    );

    let settings = gifski::Settings {
        width: Some(width),
        height: Some(height),
        quality: 90,
        fast: true,
        repeat: gifski::Repeat::Infinite,
    };

    let (collector, writer) = gifski::new(settings)
        .map_err(|e| anyhow::anyhow!("Failed to create gifski encoder: {}", e))?;

    // フレームデータを Collector に投入するスレッド
    let frames_owned: Vec<(Vec<u8>, u32, u32)> = frames.to_vec();
    let collector_thread = std::thread::spawn(move || -> Result<()> {
        for (i, (data, w, h)) in frames_owned.iter().enumerate() {
            // RGBA u8 スライスを RGBA8 ピクセル列に変換
            let pixels: Vec<rgb::RGBA8> = data
                .chunks_exact(4)
                .map(|c| rgb::RGBA8::new(c[0], c[1], c[2], c[3]))
                .collect();
            let frame = imgref::ImgVec::new(pixels, *w as usize, *h as usize);
            let timestamp = i as f64 / fps as f64;
            collector
                .add_frame_rgba(i, frame, timestamp)
                .map_err(|e| anyhow::anyhow!("Failed to add frame {}: {}", i, e))?;
        }
        // Collector をドロップして Writer に完了を通知
        drop(collector);
        Ok(())
    });

    // gifski 内部の LZW スレッドが大きなフレームでスタックオーバーフローするため、
    // Writer を十分なスタックサイズのスレッドで実行する (8 MB)
    let output_path_owned = output_path.to_path_buf();
    let writer_thread = std::thread::Builder::new()
        .name("gifski-writer".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || -> Result<()> {
            let file = std::fs::File::create(&output_path_owned)
                .with_context(|| format!("Failed to create {}", output_path_owned.display()))?;
            writer
                .write(file, &mut gifski::progress::NoProgress {})
                .map_err(|e| anyhow::anyhow!("gifski write failed: {}", e))?;
            Ok(())
        })
        .context("Failed to spawn gifski writer thread")?;

    // 両スレッドの完了を待つ
    collector_thread
        .join()
        .map_err(|_| anyhow::anyhow!("Collector thread panicked"))??;
    writer_thread
        .join()
        .map_err(|_| anyhow::anyhow!("Writer thread panicked"))??;

    log::info!("GIF encoding complete: {}", output_path.display());
    Ok(())
}

/// Encode frames as WebM (VP9) using FFmpeg.
pub fn encode_webm(
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
        "Encoding WebM: {}x{}, {} frames @ {} fps",
        width,
        height,
        frames.len(),
        fps
    );

    let temp_dir = output_path.parent().unwrap_or(Path::new("."));
    let raw_path = temp_dir.join("_temp_frames_webm.raw");

    let mut raw_data = Vec::new();
    for (data, _, _) in frames {
        raw_data.extend_from_slice(data);
    }
    std::fs::write(&raw_path, &raw_data)
        .context("Failed to write temp raw frames")?;

    let vf = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

    // First pass at CRF 30
    let mut crf = 30u32;
    loop {
        let _ = std::fs::remove_file(output_path);
        let mut webm_cmd = cpu_throttled_command(ffmpeg_path);
        let result = webm_cmd
            .args([
                "-y",
                "-f", "rawvideo",
                "-pix_fmt", "rgba",
                "-s", &format!("{}x{}", width, height),
                "-r", &fps.to_string(),
                "-i", &raw_path.to_string_lossy(),
                "-vf", vf,
                "-c:v", "libvpx-vp9",
                "-pix_fmt", "yuv420p",
                "-crf", &crf.to_string(),
                "-b:v", "0",
                &output_path.to_string_lossy(),
            ])
            .output()
            .context("Failed to run ffmpeg for WebM")?;

        if !result.status.success() {
            let _ = std::fs::remove_file(&raw_path);
            let stderr = String::from_utf8_lossy(&result.stderr);
            anyhow::bail!("FFmpeg WebM encoding failed: {}", stderr);
        }

        if let Some(max_size) = max_size_bytes {
            let file_size = std::fs::metadata(output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            if file_size > max_size && crf < 55 {
                crf += 5;
                log::info!(
                    "WebM size {} exceeds limit {}, retrying at CRF {}",
                    file_size,
                    max_size,
                    crf
                );
                continue;
            }
        }
        break;
    }

    let _ = std::fs::remove_file(&raw_path);
    Ok(())
}

/// image クレート (NeuQuant) による高速 GIF エンコード。
/// プロセス起動ゼロ、シンプルな量子化で爆速。
pub fn encode_gif_fast(
    frames: &[(Vec<u8>, u32, u32)],
    fps: u32,
    output_path: &Path,
) -> Result<()> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame};

    if frames.is_empty() {
        anyhow::bail!("No frames to encode");
    }

    let (_, width, height) = &frames[0];
    let width = *width;
    let height = *height;

    log::info!(
        "Encoding GIF (fast/NeuQuant): {}x{}, {} frames @ {} fps",
        width,
        height,
        frames.len(),
        fps
    );

    let file = std::fs::File::create(output_path)
        .with_context(|| format!("Failed to create {}", output_path.display()))?;
    let mut encoder = GifEncoder::new_with_speed(file, 10); // speed 1-30, 10=fast
    encoder
        .set_repeat(Repeat::Infinite)
        .context("Failed to set GIF repeat")?;

    // GIF のフレーム遅延は 1/100 秒単位
    let delay_cs = (100.0 / fps as f64).round() as u32;
    let delay = Delay::from_numer_denom_ms(delay_cs * 10, 1);

    let gif_frames: Vec<Frame> = frames
        .iter()
        .map(|(data, w, h)| {
            let buf: RgbaImage = ImageBuffer::from_raw(*w, *h, data.clone())
                .expect("Failed to create image buffer");
            Frame::from_parts(buf, 0, 0, delay.clone())
        })
        .collect();

    encoder
        .encode_frames(gif_frames)
        .context("Failed to encode GIF frames")?;

    log::info!("GIF encoding (fast) complete: {}", output_path.display());
    Ok(())
}

/// Encode video in the given format.
pub fn encode_video(
    ffmpeg_path: &Path,
    frames: &[(Vec<u8>, u32, u32)],
    fps: u32,
    output_path: &Path,
    max_size_bytes: Option<u64>,
    format: &str,
    gif_mode: GifQualityMode,
) -> Result<()> {
    match format.to_uppercase().as_str() {
        "MP4" => encode_mp4(ffmpeg_path, frames, fps, output_path, max_size_bytes),
        "GIF" => match gif_mode {
            GifQualityMode::Quality => encode_gif(ffmpeg_path, frames, fps, output_path, max_size_bytes),
            GifQualityMode::Fast => encode_gif_fast(frames, fps, output_path),
        },
        "WEBM" => encode_webm(ffmpeg_path, frames, fps, output_path, max_size_bytes),
        _ => anyhow::bail!("Unsupported video format: {}", format),
    }
}
