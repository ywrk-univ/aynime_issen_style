// Hide console window in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod config;
mod overlay;
mod processing;

use crate::capture::anime_title;
use crate::capture::screen::ScreenCapturer;
use crate::config::AppConfig;
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
    /// Current format name (e.g. "PNG", "GIF", "MP4")
    export_format: String,

    encode_status: Arc<Mutex<EncodeStatus>>,

    capture_exclusion_applied: bool,
    last_capture: Option<CapturedImage>,
    status_message: String,

    region_border: RegionBorder,
    capture_on_select: bool,

    config: AppConfig,
    current_tab: AppTab,

    /// Detected anime title from window titles (auto-updated)
    detected_anime: String,
}

#[derive(Clone, Copy, PartialEq)]
enum AppTab {
    Main,
    Settings,
    Status,
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

        let config = AppConfig::load();

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
            record_fps: config.default_fps,
            last_frame_time: None,
            output_dir: ensure_tools::default_output_dir(),
            ffmpeg_path,
            max_file_size_mb: config.default_max_size_mb,
            export_format: config.default_format.clone(),
            encode_status: Arc::new(Mutex::new(EncodeStatus::Idle)),
            capture_exclusion_applied: false,
            last_capture: None,
            status_message: String::new(),
            region_border: RegionBorder::new(),
            capture_on_select: true,
            config,
            current_tab: AppTab::Main,
            detected_anime: String::new(),
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
                let mut fd = egui::FontData::from_owned(font_data);
                fd.tweak.y_offset_factor = 0.1; // shift glyphs down to align with UI elements
                fonts.font_data.insert("japanese".to_owned(), fd.into());
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

    fn is_video_format(&self) -> bool {
        self.config.is_video_format(&self.export_format)
    }

    fn schedule_capturer_reinit(&mut self) {
        self.capturer = None;
        self.capturer_reinit_after = Some(Instant::now() + std::time::Duration::from_millis(600));
    }

    /// Scan window titles and update detected anime name.
    fn refresh_anime_title(&mut self) {
        if let Some(result) = anime_title::find_anime_title() {
            if self.detected_anime != result.title {
                log::info!("Anime detected: {}", result.title);
                self.detected_anime = result.title;
            }
        }
    }

    // ── Region selection (Win32 native) ─────────────────────────────

    fn do_region_selection(&mut self) {
        self.refresh_anime_title();
        // The Win32 overlay uses BitBlt (captures desktop directly via GDI).
        // Drop the DXGI capturer first to avoid conflicts, then show overlay.
        self.capturer = None;

        let do_capture = self.capture_on_select && !self.is_video_format();

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
        self.refresh_anime_title();
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
        let filestem = make_filename_stem(&self.detected_anime);

        let ext = export::format_extension(&self.export_format);
        let p = self
            .output_dir
            .join(format!("{}.{}", filestem, ext));
        let path = match export::save_still(
            &capture.rgba_data,
            capture.width,
            capture.height,
            &p,
            &self.export_format,
        ) {
            Ok(()) => Some(p),
            Err(e) => {
                self.status_message = format!("収納に失敗: {}", e);
                None
            }
        };

        if let Some(p) = path {
            self.status_message = format!("クリップボードに「収納」しました ({})", p.display());
        }
    }

    // ── Recording ───────────────────────────────────────────────────

    fn start_recording(&mut self) {
        self.refresh_anime_title();
        self.is_recording = true;
        self.recorded_frames.clear();
        self.record_start = Some(Instant::now());
        self.last_frame_time = None;
        log::info!(
            "Recording started. Region: {:?}, Screen: {:?}",
            self.region,
            self.screen_size
        );
        self.status_message =
            "キンキンキンキンキンキンキンキンキンキンキンキンキンキンキンキン！".to_string();
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
        let format = self.export_format.clone();
        let ext = export::format_extension(&format).to_string();
        let status = self.encode_status.clone();

        let _ = std::fs::create_dir_all(&output_dir);
        let filestem = make_filename_stem(&self.detected_anime);

        *status.lock().unwrap() =
            EncodeStatus::Encoding(format!("収納中... ({}フレーム)", frame_count));
        self.status_message = format!("収納中... ({}フレーム)", frame_count);

        std::thread::spawn(move || {
            let p = output_dir.join(format!("{}.{}", filestem, ext));
            let result =
                export::encode_video(&ffmpeg_path, &frames, fps, &p, Some(max_bytes), &format)
                    .map(|_| p);

            let mut s = status.lock().unwrap();
            match result {
                Ok(p) => {
                    log::info!("Encode complete: {}", p.display());
                    // Copy encoded file to clipboard
                    if let Err(e) = clipboard::copy_file_to_clipboard(&p) {
                        log::warn!("Failed to copy file to clipboard: {}", e);
                    }
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
            // ── Tab bar ─────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, AppTab::Main, "「一閃」");
                ui.selectable_value(&mut self.current_tab, AppTab::Settings, "設定");
                ui.selectable_value(&mut self.current_tab, AppTab::Status, "ステータス");
            });
            ui.separator();

            match self.current_tab {
                AppTab::Main => self.ui_tab_main(ui, ctx),
                AppTab::Settings => self.ui_tab_settings(ui),
                AppTab::Status => self.ui_tab_status(ui),
            }
        });
    }
}

// ─── Tab UIs ────────────────────────────────────────────────────────

impl AynimeApp {
    fn ui_tab_main(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

        if !self.detected_anime.is_empty() {
            ui.label(format!("検出: {}", self.detected_anime));
        }

        ui.separator();

        // ── Format selection ────────────────────────────────────
        ui.horizontal_wrapped(|ui| {
            ui.label("形式:");
            for fmt in &self.config.all_formats() {
                let selected = self.export_format.eq_ignore_ascii_case(fmt);
                if ui.selectable_label(selected, fmt).clicked() {
                    self.export_format = fmt.clone();
                }
            }
        });

        if self.is_video_format() {
            ui.horizontal(|ui| {
                ui.label("FPS:");
                let mut fps = self.record_fps as f32;
                if ui.add(egui::Slider::new(&mut fps, 5.0..=30.0)).changed() {
                    self.record_fps = fps as u32;
                }
            });
            ui.horizontal(|ui| {
                ui.label("最大サイズ:");
                for &preset in &self.config.max_size_presets_mb {
                    let label = format!("{:.0}MB", preset);
                    let selected = (self.max_file_size_mb - preset).abs() < 0.1;
                    if ui.selectable_label(selected, &label).clicked() {
                        self.max_file_size_mb = preset;
                    }
                }
            });
        }

        ui.separator();

        // ── Capture / Record buttons ────────────────────────────
        let is_encoding = matches!(
            *self.encode_status.lock().unwrap(),
            EncodeStatus::Encoding(_)
        );
        let is_still = !self.is_video_format();

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
                            egui::Button::new("キンキンキンキンキンキンキンキンキンキンキンキンキンキンキンキン！").min_size(egui::vec2(180.0, 40.0)),
                        )
                        .clicked()
                    {
                        self.start_recording();
                    }
                } else {
                    if ui
                        .add_sized([180.0, 40.0], egui::Button::new("停止・収納"))
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

        // ── Status message ──────────────────────────────────────
        if !self.status_message.is_empty() {
            ui.separator();
            ui.label(&self.status_message);
        }
    }

    fn ui_tab_settings(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("設定");
            ui.separator();

            // ── Still formats ───────────────────────────────────
            ui.label("静止画フォーマット:");
            let all_still = ["PNG", "WebP", "JPEG", "BMP"];
            ui.horizontal_wrapped(|ui| {
                for fmt in &all_still {
                    let enabled = self
                        .config
                        .still_formats
                        .iter()
                        .any(|f| f.eq_ignore_ascii_case(fmt));
                    let mut checked = enabled;
                    if ui.checkbox(&mut checked, *fmt).changed() {
                        if checked && !enabled {
                            self.config.still_formats.push(fmt.to_string());
                        } else if !checked {
                            self.config
                                .still_formats
                                .retain(|f| !f.eq_ignore_ascii_case(fmt));
                        }
                    }
                }
            });

            ui.add_space(4.0);

            // ── Video formats ───────────────────────────────────
            ui.label("動画フォーマット:");
            let all_video = ["GIF", "MP4", "WebM"];
            ui.horizontal_wrapped(|ui| {
                for fmt in &all_video {
                    let enabled = self
                        .config
                        .video_formats
                        .iter()
                        .any(|f| f.eq_ignore_ascii_case(fmt));
                    let mut checked = enabled;
                    if ui.checkbox(&mut checked, *fmt).changed() {
                        if checked && !enabled {
                            self.config.video_formats.push(fmt.to_string());
                        } else if !checked {
                            self.config
                                .video_formats
                                .retain(|f| !f.eq_ignore_ascii_case(fmt));
                        }
                    }
                }
            });

            ui.add_space(4.0);
            ui.separator();

            // ── Default format ──────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("デフォルト形式:");
                for fmt in &self.config.all_formats() {
                    let selected = self.config.default_format.eq_ignore_ascii_case(fmt);
                    if ui.selectable_label(selected, fmt).clicked() {
                        self.config.default_format = fmt.clone();
                    }
                }
            });

            // ── Default FPS ─────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("デフォルトFPS:");
                let mut fps = self.config.default_fps as f32;
                if ui.add(egui::Slider::new(&mut fps, 5.0..=30.0)).changed() {
                    self.config.default_fps = fps as u32;
                }
            });

            // ── Default max size ────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("デフォルト最大サイズ:");
                let mut size = self.config.default_max_size_mb;
                if ui
                    .add(egui::Slider::new(&mut size, 1.0..=50.0).suffix(" MB"))
                    .changed()
                {
                    self.config.default_max_size_mb = size;
                }
            });

            ui.add_space(4.0);
            ui.separator();

            // ── Output directory ────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("出力先:");
                let mut dir_str = self.output_dir.to_string_lossy().to_string();
                if ui.text_edit_singleline(&mut dir_str).changed() {
                    self.output_dir = PathBuf::from(dir_str);
                }
            });

            ui.horizontal(|ui| {
                if ui.button("設定フォルダを開く").clicked() {
                    if let Some(dir) = config::AppConfig::config_dir() {
                        let _ = std::process::Command::new("explorer").arg(dir).spawn();
                    }
                }
                if ui.button("出力フォルダを開く").clicked() {
                    let _ = std::fs::create_dir_all(&self.output_dir);
                    let _ = std::process::Command::new("explorer")
                        .arg(&self.output_dir)
                        .spawn();
                }
            });

            ui.add_space(8.0);

            // ── Save button ─────────────────────────────────────
            if ui
                .add_sized([280.0, 32.0], egui::Button::new("設定を保存"))
                .clicked()
            {
                match self.config.save() {
                    Ok(()) => self.status_message = "設定を保存しました".to_string(),
                    Err(e) => self.status_message = format!("設定の保存に失敗: {}", e),
                }
            }
        });
    }

    fn ui_tab_status(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            // ── バージョン ───────────────────────────────────────
            ui.heading("バージョン");
            ui.label(format!("{} v{}", APP_TITLE, env!("CARGO_PKG_VERSION")));
            ui.label(format!(
                "画面: {}x{}",
                self.screen_size.0, self.screen_size.1
            ));
            ui.label(format!(
                "設定: {}",
                config::AppConfig::config_path_display()
            ));
            ui.label(format!("出力: {}", self.output_dir.display()));

            ui.separator();

            // ── 本家様 ───────────────────────────────────────

            ui.heading("えぃにめ一閃流奥義「一閃」（本家様）");
            ui.horizontal(|ui| {
                ui.label("GitHub:");
                ui.hyperlink("https://github.com/Nu-Pan/aynime_issen_style");
            });

            ui.separator();

            // ── ライセンス ───────────────────────────────────────
            ui.heading("ライセンス");
            ui.label("えぃにめ一閃流奥義「一閃 改」は MIT ライセンスで公開しています。");
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("license_scroll")
                .max_height(250.0)
                .show(ui, |ui| {
                    ui.monospace(MIT_LICENSE_TEXT);
                });

            ui.add_space(4.0);
            ui.label("本ソフトウェアは以下のソフトウェアをサブプロセスとして利用します:");
            ui.label("  FFmpeg (GPL v3) - https://ffmpeg.org/");
            ui.add_space(4.0);
            ui.label("主要な依存ライブラリ:");
            ui.label("  eframe/egui (MIT/Apache-2.0) - GUI");
            ui.label("  windows-rs (MIT/Apache-2.0) - Win32 API");
            ui.label("  image (MIT/Apache-2.0) - 画像処理");
        });
    }
}

const MIT_LICENSE_TEXT: &str = "\
Copyright 2025 NU-Pan

Permission is hereby granted, free of charge, to any person obtaining a copy \
of this software and associated documentation files (the \"Software\"), to deal \
in the Software without restriction, including without limitation the rights \
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell \
copies of the Software, and to permit persons to whom the Software is \
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in \
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR \
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, \
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE \
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER \
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, \
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN \
THE SOFTWARE.";

/// Generate a timestamp in the format used by the original app: YYYY-MM-DD_HH-MM-SS-mmm
fn nime_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();

    // Convert to date/time components (UTC — matches original behavior)
    let secs_per_day = 86400u64;
    let days = total_secs / secs_per_day;
    let day_secs = (total_secs % secs_per_day) as u32;
    let hour = day_secs / 3600;
    let min = (day_secs % 3600) / 60;
    let sec = day_secs % 60;

    // Days since epoch to Y-M-D (simplified Gregorian)
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}-{:03}",
        year, month, day, hour, min, sec, millis
    )
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Epoch is 1970-01-01
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Generate a filename stem: "{anime_name}__{timestamp}" or "capture__{timestamp}"
/// Matches the original Python app's naming convention.
fn make_filename_stem(anime_title: &str) -> String {
    let ts = nime_timestamp();
    if anime_title.is_empty() {
        format!("capture__{}", ts)
    } else {
        let sanitized = anime_title::sanitize_for_filename(anime_title);
        let truncated: String = sanitized.chars().take(80).collect();
        format!("{}__{}", truncated, ts)
    }
}
