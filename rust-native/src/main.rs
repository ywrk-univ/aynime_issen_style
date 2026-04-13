mod capture;
mod overlay;
mod processing;

use crate::capture::screen::ScreenCapturer;
use crate::overlay::border::RegionBorder;
use crate::overlay::selection;
use crate::overlay::window::apply_capture_exclusion_to_egui_window;
use crate::processing::clipboard;
use crate::processing::ensure_tools;
use crate::processing::export;
use crate::processing::region::CaptureRegion;

use anyhow::Result;
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const APP_TITLE: &str = "えぃにめ一閃流奥義「一閃 改」";

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting {}", APP_TITLE);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([400.0, 620.0])
            .with_always_on_top()
            .with_decorations(true)
            .with_transparent(false),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(AynimeApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}

// ─── Encode thread status ───────────────────────────────────────────

#[derive(Clone)]
enum EncodeStatus {
    Idle,
    Encoding(String),
    Done(String),
    Error(String),
}

// ─── App state ──────────────────────────────────────────────────────

struct AynimeApp {
    capturer: Option<ScreenCapturer>,
    capture_error: Option<String>,
    capturer_reinit_after: Option<Instant>,

    region: CaptureRegion,
    screen_size: (u32, u32),

    is_recording: bool,
    recorded_frames: Vec<(Vec<u8>, u32, u32)>,
    record_start: Option<Instant>,
    record_fps: u32,
    last_frame_time: Option<Instant>,

    output_dir: PathBuf,
    ffmpeg_path: PathBuf,
    max_file_size_mb: f32,
    export_format: ExportFormat,

    encode_status: Arc<Mutex<EncodeStatus>>,

    capture_exclusion_applied: bool,
    last_capture: Option<CapturedImage>,
    status_message: String,

    region_border: RegionBorder,
    /// When true, still capture is done at region selection time
    capture_on_select: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ExportFormat {
    Png,
    WebP,
    Mp4,
    Gif,
}

struct CapturedImage {
    texture: Option<egui::TextureHandle>,
    width: u32,
    height: u32,
    rgba_data: Vec<u8>,
}

// ─── App implementation ─────────────────────────────────────────────

impl AynimeApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::setup_japanese_fonts(&cc.egui_ctx);

        let capturer = match ScreenCapturer::new() {
            Ok(c) => {
                log::info!("Screen capturer initialized: {:?}", c.dimensions());
                Some(c)
            }
            Err(e) => {
                log::error!("Failed to init screen capturer: {}", e);
                None
            }
        };

        let screen_size = capturer
            .as_ref()
            .map(|c| c.dimensions())
            .unwrap_or((1920, 1080));

        let ffmpeg_path = match ensure_tools::ensure_ffmpeg() {
            Ok(p) => {
                log::info!("ffmpeg ready: {}", p.display());
                p
            }
            Err(e) => {
                log::warn!("ffmpeg not available yet: {}", e);
                PathBuf::from("ffmpeg")
            }
        };

        Self {
            capture_error: if capturer.is_none() {
                Some(
                    "キャプチャに失敗。\nキャプチャ対象のディスプレイの選択を忘れている？"
                        .to_string(),
                )
            } else {
                None
            },
            capturer,
            capturer_reinit_after: None,
            region: CaptureRegion::new(0, 0, screen_size.0, screen_size.1),
            screen_size,
            is_recording: false,
            recorded_frames: Vec::new(),
            record_start: None,
            record_fps: 15,
            last_frame_time: None,
            output_dir: PathBuf::from("./output"),
            ffmpeg_path,
            max_file_size_mb: 8.0,
            export_format: ExportFormat::Png,
            encode_status: Arc::new(Mutex::new(EncodeStatus::Idle)),
            capture_exclusion_applied: false,
            last_capture: None,
            status_message: String::new(),
            region_border: RegionBorder::new(),
            capture_on_select: true,
        }
    }

    fn setup_japanese_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let font_paths = [
            "C:\\Windows\\Fonts\\YuGothM.ttc",
            "C:\\Windows\\Fonts\\yugothic.ttf",
            "C:\\Windows\\Fonts\\meiryo.ttc",
            "C:\\Windows\\Fonts\\msgothic.ttc",
        ];

        for path in &font_paths {
            if let Ok(font_data) = std::fs::read(path) {
                log::info!("Loaded Japanese font: {}", path);
                fonts.font_data.insert(
                    "japanese".to_owned(),
                    egui::FontData::from_owned(font_data).into(),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "japanese".to_owned());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "japanese".to_owned());
                break;
            }
        }
        ctx.set_fonts(fonts);
    }

    // ── Capturer management ─────────────────────────────────────────

    fn ensure_capturer(&mut self) -> bool {
        if self.capturer.is_some() {
            return true;
        }
        if let Some(after) = self.capturer_reinit_after {
            if Instant::now() < after {
                return false;
            }
            self.capturer_reinit_after = None;
        }
        match ScreenCapturer::new() {
            Ok(c) => {
                log::info!("Screen capturer (re)initialized: {:?}", c.dimensions());
                self.capturer = Some(c);
                true
            }
            Err(e) => {
                log::error!("Failed to init screen capturer: {}", e);
                self.capturer_reinit_after =
                    Some(Instant::now() + std::time::Duration::from_millis(300));
                false
            }
        }
    }

    fn schedule_capturer_reinit(&mut self) {
        self.capturer = None;
        self.capturer_reinit_after = Some(Instant::now() + std::time::Duration::from_millis(600));
    }

    // ── Region selection (Win32 native) ─────────────────────────────

    fn do_region_selection(&mut self) {
        // The Win32 overlay uses BitBlt (captures desktop directly via GDI).
        // Drop the DXGI capturer first to avoid conflicts, then show overlay.
        self.capturer = None;

        let do_capture = self.capture_on_select
            && matches!(self.export_format, ExportFormat::Png | ExportFormat::WebP);

        log::info!(
            "Opening native selection overlay... (capture={})",
            do_capture
        );
        let result = selection::show_selection_overlay(do_capture);

        // After overlay closes, schedule DXGI re-init
        self.schedule_capturer_reinit();

        if let Some(sel) = result {
            let x = sel.x.max(0) as u32;
            let y = sel.y.max(0) as u32;
            let w = sel.width.max(2) as u32;
            let h = sel.height.max(2) as u32;

            // If capture-on-select (即一閃), capture and reset region to fullscreen
            if let Some(rgba) = sel.rgba_data {
                if let Err(e) = clipboard::copy_rgba_to_clipboard(&rgba, w, h) {
                    log::warn!("Clipboard copy failed: {}", e);
                }
                self.last_capture = Some(CapturedImage {
                    texture: None,
                    width: w,
                    height: h,
                    rgba_data: rgba,
                });
                // Reset to fullscreen — don't keep the selection region
                self.region = CaptureRegion::new(0, 0, self.screen_size.0, self.screen_size.1);
                self.region_border.hide();
                log::info!("即一閃: {}x{} @ ({}, {})", w, h, x, y);
                self.status_message = format!("クリップボードに「収納」しました ({}x{})", w, h);
            } else {
                // Normal selection — keep region
                self.region = CaptureRegion::new(x, y, w, h);
                self.region_border
                    .update(x as i32, y as i32, w as i32, h as i32);
                log::info!("Region set: {}x{} @ ({}, {})", w, h, x, y);
                self.status_message = format!("構え完了: {}x{} @ ({}, {})", w, h, x, y);
            }
        } else {
            self.status_message = "構えをキャンセルしました".to_string();
        }
    }

    // ── Capture ─────────────────────────────────────────────────────

    fn do_still_capture(&mut self) {
        if !self.ensure_capturer() {
            self.status_message = "準備中...".to_string();
            return;
        }

        let Some(capturer) = &mut self.capturer else {
            return;
        };

        for _ in 0..3 {
            match capturer.capture_frame() {
                Ok(Some(frame)) => {
                    let (rgba, w, h) = self.region.extract_from_frame(&frame);
                    // Copy to clipboard
                    if let Err(e) = clipboard::copy_rgba_to_clipboard(&rgba, w, h) {
                        log::warn!("Clipboard copy failed: {}", e);
                    }
                    self.last_capture = Some(CapturedImage {
                        texture: None,
                        width: w,
                        height: h,
                        rgba_data: rgba,
                    });
                    self.status_message = format!("クリップボードに「収納」しました ({}x{})", w, h);
                    return;
                }
                Ok(None) => continue,
                Err(_) => {
                    self.schedule_capturer_reinit();
                    self.status_message = "準備中...".to_string();
                    return;
                }
            }
        }
        self.status_message =
            "キャプチャに失敗。\nキャプチャ対象のディスプレイの選択を忘れている？".to_string();
    }

    fn do_save(&mut self) {
        let Some(capture) = &self.last_capture else {
            self.status_message = "キャプチャ画像なし".to_string();
            return;
        };

        let _ = std::fs::create_dir_all(&self.output_dir);
        let timestamp = simple_timestamp();

        let path = match self.export_format {
            ExportFormat::Png => {
                let p = self.output_dir.join(format!("capture_{}.png", timestamp));
                export::save_as_png(&capture.rgba_data, capture.width, capture.height, &p)
                    .map(|_| p)
                    .ok()
            }
            ExportFormat::WebP => {
                let p = self.output_dir.join(format!("capture_{}.webp", timestamp));
                export::save_as_webp(&capture.rgba_data, capture.width, capture.height, &p, 85)
                    .map(|_| p)
                    .ok()
            }
            _ => {
                self.status_message = "静止画はPNG/WebPで収納してください".to_string();
                None
            }
        };

        if let Some(p) = path {
            self.status_message = format!("クリップボードに「収納」しました ({})", p.display());
        }
    }

    // ── Recording ───────────────────────────────────────────────────

    fn start_recording(&mut self) {
        self.is_recording = true;
        self.recorded_frames.clear();
        self.record_start = Some(Instant::now());
        self.last_frame_time = None;
        log::info!(
            "Recording started. Region: {:?}, Screen: {:?}",
            self.region,
            self.screen_size
        );
        self.status_message = "キンキン中...".to_string();
    }

    fn stop_recording(&mut self) {
        self.is_recording = false;
        let frame_count = self.recorded_frames.len();

        if frame_count == 0 {
            self.status_message = "動画の保存には最低でも 2 フレーム必要だよ".to_string();
            return;
        }

        let frames = std::mem::take(&mut self.recorded_frames);
        let ffmpeg_path = self.ffmpeg_path.clone();
        let output_dir = self.output_dir.clone();
        let fps = self.record_fps;
        let max_bytes = (self.max_file_size_mb * 1024.0 * 1024.0) as u64;
        let format = self.export_format;
        let status = self.encode_status.clone();

        let _ = std::fs::create_dir_all(&output_dir);
        let timestamp = simple_timestamp();

        *status.lock().unwrap() =
            EncodeStatus::Encoding(format!("収納中... ({}フレーム)", frame_count));
        self.status_message = format!("収納中... ({}フレーム)", frame_count);

        std::thread::spawn(move || {
            let result = match format {
                ExportFormat::Mp4 => {
                    let p = output_dir.join(format!("capture_{}.mp4", timestamp));
                    export::encode_mp4(&ffmpeg_path, &frames, fps, &p, Some(max_bytes)).map(|_| p)
                }
                ExportFormat::Gif => {
                    let p = output_dir.join(format!("capture_{}.gif", timestamp));
                    export::encode_gif(&ffmpeg_path, &frames, fps, &p, Some(max_bytes)).map(|_| p)
                }
                _ => Err(anyhow::anyhow!("Unsupported format for video")),
            };

            let mut s = status.lock().unwrap();
            match result {
                Ok(p) => {
                    log::info!("Encode complete: {}", p.display());
                    *s = EncodeStatus::Done(format!(
                        "クリップボードに「収納」しました: {}",
                        p.display()
                    ));
                }
                Err(e) => {
                    log::error!("Encode error: {}", e);
                    *s = EncodeStatus::Error(format!("収納に失敗: {}", e));
                }
            }
        });
    }

    fn record_frame(&mut self) {
        if !self.ensure_capturer() {
            return;
        }
        let Some(capturer) = &mut self.capturer else {
            return;
        };

        let now = Instant::now();
        let frame_interval = std::time::Duration::from_secs_f64(1.0 / self.record_fps as f64);

        if let Some(last) = self.last_frame_time {
            if now.duration_since(last) < frame_interval {
                return;
            }
        }

        if let Ok(Some(frame)) = capturer.capture_frame() {
            let (rgba, w, h) = self.region.extract_from_frame(&frame);
            if w >= 2 && h >= 2 {
                self.recorded_frames.push((rgba, w, h));
                self.last_frame_time = Some(now);
            }
        }
    }
}

// ─── eframe::App ────────────────────────────────────────────────────

impl eframe::App for AynimeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply WDA_EXCLUDEFROMCAPTURE once
        if !self.capture_exclusion_applied {
            if let Ok(_) = apply_capture_exclusion_to_egui_window(APP_TITLE) {
                self.capture_exclusion_applied = true;
                log::info!("Capture exclusion applied");
            }
        }

        // Try capturer re-init if pending (non-blocking)
        if self.capturer.is_none() && self.capturer_reinit_after.is_some() {
            self.ensure_capturer();
            if self.capturer.is_none() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

        // Poll encode thread status
        {
            let mut status = self.encode_status.lock().unwrap();
            match &*status {
                EncodeStatus::Done(msg) | EncodeStatus::Error(msg) => {
                    self.status_message = msg.clone();
                    *status = EncodeStatus::Idle;
                }
                EncodeStatus::Encoding(msg) => {
                    self.status_message = msg.clone();
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                EncodeStatus::Idle => {}
            }
        }

        // Recording loop
        if self.is_recording {
            self.record_frame();
            ctx.request_repaint();
        }

        // ── Main panel ──────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(APP_TITLE);
            ui.separator();

            if let Some(err) = &self.capture_error {
                ui.colored_label(egui::Color32::RED, err.as_str());
                ui.separator();
            }

            // ── Region selection ────────────────────────────────────
            ui.horizontal(|ui| {
                if ui
                    .add_sized([120.0, 32.0], egui::Button::new("構え"))
                    .clicked()
                {
                    self.do_region_selection();
                }
                if ui
                    .add_sized([120.0, 32.0], egui::Button::new("構え解除"))
                    .clicked()
                {
                    self.region = CaptureRegion::new(0, 0, self.screen_size.0, self.screen_size.1);
                    self.region_border.hide();
                    self.status_message = "構えを解除しました".to_string();
                }
                ui.checkbox(&mut self.capture_on_select, "即一閃");
            });

            ui.label(format!(
                "範囲: {}x{} @ ({}, {})",
                self.region.width, self.region.height, self.region.x, self.region.y
            ));

            ui.separator();

            // ── Export settings ─────────────────────────────────────
            ui.collapsing("出力設定", |ui| {
                ui.horizontal(|ui| {
                    ui.label("形式:");
                    ui.selectable_value(&mut self.export_format, ExportFormat::Png, "PNG");
                    ui.selectable_value(&mut self.export_format, ExportFormat::WebP, "WebP");
                    ui.selectable_value(&mut self.export_format, ExportFormat::Mp4, "MP4");
                    ui.selectable_value(&mut self.export_format, ExportFormat::Gif, "GIF");
                });

                if matches!(self.export_format, ExportFormat::Mp4 | ExportFormat::Gif) {
                    ui.horizontal(|ui| {
                        ui.label("FPS:");
                        let mut fps = self.record_fps as f32;
                        if ui.add(egui::Slider::new(&mut fps, 5.0..=30.0)).changed() {
                            self.record_fps = fps as u32;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("最大サイズ:");
                        ui.add(
                            egui::Slider::new(&mut self.max_file_size_mb, 1.0..=50.0).suffix(" MB"),
                        );
                    });
                }

                ui.horizontal(|ui| {
                    ui.label("出力先:");
                    let mut dir_str = self.output_dir.to_string_lossy().to_string();
                    if ui.text_edit_singleline(&mut dir_str).changed() {
                        self.output_dir = PathBuf::from(dir_str);
                    }
                });
            });

            ui.separator();

            // ── Capture / Record buttons ────────────────────────────
            let is_encoding = matches!(
                *self.encode_status.lock().unwrap(),
                EncodeStatus::Encoding(_)
            );
            let is_still = matches!(self.export_format, ExportFormat::Png | ExportFormat::WebP);

            if is_still {
                ui.horizontal(|ui| {
                    if ui
                        .add_sized([180.0, 40.0], egui::Button::new("「一閃」"))
                        .clicked()
                    {
                        self.do_still_capture();
                    }
                    if self.last_capture.is_some() {
                        if ui
                            .add_sized([120.0, 40.0], egui::Button::new("収納"))
                            .clicked()
                        {
                            self.do_save();
                        }
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    if !self.is_recording {
                        if ui
                            .add_enabled(
                                !is_encoding,
                                egui::Button::new("キンキン開始").min_size(egui::vec2(180.0, 40.0)),
                            )
                            .clicked()
                        {
                            self.start_recording();
                        }
                    } else {
                        if ui
                            .add_sized([180.0, 40.0], egui::Button::new("キンキン停止・収納"))
                            .clicked()
                        {
                            self.stop_recording();
                        }

                        if let Some(start) = self.record_start {
                            let elapsed = start.elapsed().as_secs_f32();
                            ui.label(format!(
                                "{:.1}秒 / {}フレーム",
                                elapsed,
                                self.recorded_frames.len()
                            ));
                        }
                    }
                });

                if is_encoding {
                    ui.spinner();
                }
            }

            ui.separator();

            // ── Preview ─────────────────────────────────────────────
            if let Some(capture) = &mut self.last_capture {
                ui.label("プレビュー:");
                let tex = capture.texture.get_or_insert_with(|| {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [capture.width as usize, capture.height as usize],
                        &capture.rgba_data,
                    );
                    ctx.load_texture("capture_preview", color_image, egui::TextureOptions::LINEAR)
                });

                let available = ui.available_size();
                let aspect = capture.width as f32 / capture.height.max(1) as f32;
                let preview_w = available.x.min(360.0);
                let preview_h = preview_w / aspect;

                ui.image(egui::load::SizedTexture::new(
                    tex.id(),
                    egui::vec2(preview_w, preview_h),
                ));
            }

            // ── Status ──────────────────────────────────────────────
            if !self.status_message.is_empty() {
                ui.separator();
                ui.label(&self.status_message);
            }
        });
    }
}

fn simple_timestamp() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}
