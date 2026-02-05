//! UI window mode — floating HUD using egui/eframe
//!
//! Provides a frameless overlay window as an alternative to the inline terminal UI.
//! Activated via `claudio ui`. Requires the `ui` cargo feature.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Rect, Stroke, Vec2};

use crate::speech::SpeechRecognizer;

// ── Constants ────────────────────────────────────────────────────────────────

const WINDOW_WIDTH: f32 = 480.0;
const WINDOW_MIN_HEIGHT: f32 = 80.0;
const _WINDOW_MAX_HEIGHT: f32 = 400.0;
const CORNER_RADIUS: f32 = 12.0;
const PADDING: f32 = 20.0;
const FONT_SIZE: f32 = 16.0;

// Glow animation
const GLOW_LAYERS: usize = 8;
const GLOW_SPREAD: f32 = 2.5;
const GLOW_BASE_ALPHA: f32 = 50.0;
const GLOW_WAVE_SPEED: f32 = 1.5; // cycles per second
const GLOW_WAVE_AMPLITUDE: f32 = 0.4; // 0..1 modulation depth

// Text animation (matching terminal UI)
const CHAR_FADE_DELAY_MS: f32 = 20.0;
const CHAR_FADE_DURATION_MS: f32 = 1500.0;

// Colors
const GLOW_RECORDING: Color32 = Color32::from_rgb(232, 135, 90); // warm orange
const GLOW_LOADING: Color32 = Color32::from_rgb(120, 120, 130); // muted gray

const DARK_BG: Color32 = Color32::from_rgb(30, 30, 46);
const LIGHT_BG: Color32 = Color32::from_rgb(245, 245, 247);

const DARK_TEXT: Color32 = Color32::from_rgb(255, 255, 255);
const LIGHT_TEXT: Color32 = Color32::from_rgb(29, 29, 31);

const DARK_DIM: Color32 = Color32::from_rgb(100, 100, 110);
const LIGHT_DIM: Color32 = Color32::from_rgb(160, 160, 170);

// Unsettled text animates toward the settled color
const DARK_UNSETTLED: Color32 = Color32::from_rgb(120, 160, 180); // cyan
const LIGHT_UNSETTLED: Color32 = Color32::from_rgb(60, 120, 150);

const DARK_PLACEHOLDER: Color32 = Color32::from_rgb(80, 80, 90);
const LIGHT_PLACEHOLDER: Color32 = Color32::from_rgb(180, 180, 190);

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum HudState {
    Loading,
    Recording,
    Paused,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HudMode {
    Listening,
    Editing,
}

pub struct HudApp {
    // Shared state with speech recognizer
    transcription: Arc<Mutex<String>>,
    is_listening: Arc<AtomicBool>,
    is_ready: Arc<AtomicBool>,
    recognizer: Option<SpeechRecognizer>,

    // Text tracking (mirrors terminal Ui logic)
    frozen_text: String,
    current_text: String,
    stable_len: usize,
    animation_start_ms: f32,

    // UI state
    state: HudState,
    mode: HudMode,
    edit_buffer: String,
    start_time: Instant,
    should_quit: bool,
    exit_code: i32,
    final_text: Arc<Mutex<Option<String>>>,

}

impl HudApp {
    pub fn new(final_text: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            transcription: Arc::new(Mutex::new(String::new())),
            is_listening: Arc::new(AtomicBool::new(false)),
            is_ready: Arc::new(AtomicBool::new(false)),
            recognizer: None,
            frozen_text: String::new(),
            current_text: String::new(),
            stable_len: 0,
            animation_start_ms: 0.0,
            state: HudState::Loading,
            mode: HudMode::Listening,
            edit_buffer: String::new(),
            start_time: Instant::now(),
            should_quit: false,
            exit_code: 0,
            final_text,
        }
    }

    pub fn start_listening(&mut self) -> anyhow::Result<()> {
        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(
            transcription,
            is_listening,
            is_ready,
        )?);
        self.recognizer.as_mut().unwrap().start()?;
        Ok(())
    }

    fn stop_listening(&mut self) {
        if let Some(ref mut recognizer) = self.recognizer {
            recognizer.stop();
        }
        self.is_listening.store(false, Ordering::SeqCst);
    }

    fn restart(&mut self) -> anyhow::Result<()> {
        self.stop_listening();
        self.transcription.lock().unwrap().clear();
        self.start_time = Instant::now();
        self.is_ready.store(false, Ordering::SeqCst);
        self.frozen_text.clear();
        self.current_text.clear();
        self.stable_len = 0;
        self.animation_start_ms = 0.0;

        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(
            transcription,
            is_listening,
            is_ready,
        )?);
        self.recognizer.as_mut().unwrap().start()?;
        Ok(())
    }

    fn full_text(&self) -> String {
        format!("{}{}", self.frozen_text, self.current_text)
    }

    fn is_empty(&self) -> bool {
        self.frozen_text.is_empty() && self.current_text.is_empty()
    }

    /// Update text state from speech recognizer (same logic as terminal Ui::set_text)
    fn update_text(&mut self, elapsed_ms: f32) {
        if self.mode == HudMode::Editing {
            return;
        }

        let text = self.transcription.lock().unwrap().clone();

        if text == self.current_text {
            return;
        }

        let common_prefix_len = self
            .current_text
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .count();

        let new_text_len = text.chars().count();
        let new_stable_len = common_prefix_len.max(self.stable_len.min(new_text_len));

        if new_text_len > new_stable_len {
            if self.current_text.is_empty() || new_stable_len != self.stable_len {
                self.animation_start_ms = elapsed_ms;
            } else {
                let old_unstable: String = self.current_text.chars().skip(self.stable_len).collect();
                let new_unstable: String = text.chars().skip(new_stable_len).collect();

                if new_unstable.starts_with(&old_unstable) {
                    let new_chars = new_unstable.chars().count() - old_unstable.chars().count();
                    if new_chars > 0 {
                        self.animation_start_ms -= new_chars as f32 * CHAR_FADE_DELAY_MS;
                    }
                } else {
                    self.animation_start_ms = elapsed_ms;
                }
            }
        }

        self.stable_len = new_stable_len;
        self.current_text = text;
    }

    fn update_state(&mut self) {
        let is_ready = self.is_ready.load(Ordering::SeqCst);
        let is_listening = self.is_listening.load(Ordering::SeqCst);

        self.state = if !is_ready {
            HudState::Loading
        } else if is_listening {
            HudState::Recording
        } else {
            HudState::Paused
        };
    }

    // ── Colors (theme-aware) ─────────────────────────────────────────────

    fn bg_color(&self, dark: bool) -> Color32 {
        if dark { DARK_BG } else { LIGHT_BG }
    }

    fn text_color(&self, dark: bool) -> Color32 {
        if dark { DARK_TEXT } else { LIGHT_TEXT }
    }

    fn unsettled_color(&self, dark: bool) -> Color32 {
        if dark { DARK_UNSETTLED } else { LIGHT_UNSETTLED }
    }

    fn placeholder_color(&self, dark: bool) -> Color32 {
        if dark { DARK_PLACEHOLDER } else { LIGHT_PLACEHOLDER }
    }

    /// Interpolate from unsettled color toward settled text color
    fn animated_color(&self, dark: bool, progress: f32) -> Color32 {
        let from = self.unsettled_color(dark);
        let to = self.text_color(dark);
        let eased = 1.0 - (1.0 - progress).powi(3); // ease-out cubic
        Color32::from_rgb(
            lerp_u8(from.r(), to.r(), eased),
            lerp_u8(from.g(), to.g(), eased),
            lerp_u8(from.b(), to.b(), eased),
        )
    }

    // ── Glow border ──────────────────────────────────────────────────────

    fn paint_glow(&self, painter: &egui::Painter, rect: Rect, time_s: f32) {
        let (base_color, animate) = match self.state {
            HudState::Loading => (GLOW_LOADING, true),
            HudState::Recording => (GLOW_RECORDING, true),
            HudState::Paused => return, // no glow when paused
        };

        for i in (0..GLOW_LAYERS).rev() {
            let expand = GLOW_SPREAD * (i as f32 + 1.0);
            let base_alpha = GLOW_BASE_ALPHA / (i as f32 + 1.0);

            // Wave modulation: gentle pulsing brightness
            let alpha = if animate {
                let wave = (time_s * GLOW_WAVE_SPEED * std::f32::consts::TAU).sin();
                let modulation = 1.0 + wave * GLOW_WAVE_AMPLITUDE;
                (base_alpha * modulation).clamp(0.0, 255.0)
            } else {
                base_alpha
            };

            let glow_color = Color32::from_rgba_unmultiplied(
                base_color.r(),
                base_color.g(),
                base_color.b(),
                alpha as u8,
            );

            let r = CORNER_RADIUS + expand;
            painter.rect_filled(rect.expand(expand), r, glow_color);
        }
    }

    // ── Text layout ──────────────────────────────────────────────────────

    fn build_text_layout(&self, dark: bool, elapsed_ms: f32) -> LayoutJob {
        let mut job = LayoutJob {
            wrap: egui::text::TextWrapping {
                max_rows: 0, // unlimited
                ..Default::default()
            },
            ..Default::default()
        };

        let font = FontId::new(FONT_SIZE, FontFamily::Proportional);
        let settled_fmt = TextFormat {
            font_id: font.clone(),
            color: self.text_color(dark),
            ..Default::default()
        };

        // Frozen text — always settled color
        if !self.frozen_text.is_empty() {
            job.append(&self.frozen_text, 0.0, settled_fmt.clone());
        }

        // Current speech text — per-character animation
        if !self.current_text.is_empty() {
            let relative_time = elapsed_ms - self.animation_start_ms;

            for (i, ch) in self.current_text.chars().enumerate() {
                let color = if i < self.stable_len {
                    self.text_color(dark)
                } else {
                    let anim_index = i - self.stable_len;
                    let appear_time = anim_index as f32 * CHAR_FADE_DELAY_MS;
                    if relative_time < appear_time {
                        // Not visible yet — render transparent
                        Color32::TRANSPARENT
                    } else {
                        let age = relative_time - appear_time;
                        let progress = (age / CHAR_FADE_DURATION_MS).min(1.0);
                        self.animated_color(dark, progress)
                    }
                };

                let fmt = TextFormat {
                    font_id: font.clone(),
                    color,
                    ..Default::default()
                };
                let mut buf = [0u8; 4];
                job.append(ch.encode_utf8(&mut buf), 0.0, fmt);
            }
        }

        job
    }

    // ── Input handling ───────────────────────────────────────────────────

    fn handle_input(&mut self, ctx: &egui::Context) {
        match self.mode {
            HudMode::Listening => self.handle_listening_input(ctx),
            HudMode::Editing => self.handle_editing_input(ctx),
        }
    }

    fn handle_listening_input(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            // Enter → submit
            if i.key_pressed(egui::Key::Enter) {
                self.stop_listening();
                let text = self.full_text();
                *self.final_text.lock().unwrap() = Some(text);
                self.should_quit = true;
                self.exit_code = 0;
            }
            // Escape → cancel
            if i.key_pressed(egui::Key::Escape) {
                self.stop_listening();
                self.should_quit = true;
                self.exit_code = 130;
            }
            // Ctrl+D → clear and restart
            if i.modifiers.ctrl && i.key_pressed(egui::Key::D) {
                self.frozen_text.clear();
                self.current_text.clear();
                self.stable_len = 0;
                self.animation_start_ms = 0.0;
                let _ = self.restart();
            }
            // Click on text area → enter edit mode
        });
    }

    fn handle_editing_input(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            // Enter → submit edited text
            if i.key_pressed(egui::Key::Enter) && i.modifiers.command {
                self.frozen_text = self.edit_buffer.clone();
                self.current_text.clear();
                self.stable_len = 0;
                let text = self.frozen_text.clone();
                *self.final_text.lock().unwrap() = Some(text);
                self.stop_listening();
                self.should_quit = true;
                self.exit_code = 0;
            }
            // Escape → discard edits, resume listening
            if i.key_pressed(egui::Key::Escape) {
                self.mode = HudMode::Listening;
                self.transcription.lock().unwrap().clear();
                let _ = self.start_listening();
            }
        });
    }
}

impl eframe::App for HudApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let elapsed_ms = self.start_time.elapsed().as_millis() as f32;
        let time_s = self.start_time.elapsed().as_secs_f32();
        let dark = ctx.style().visuals.dark_mode;

        // Update state from recognizer
        self.update_state();
        self.update_text(elapsed_ms);

        // Handle keyboard input
        self.handle_input(ctx);

        if self.should_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Request continuous repaint for animations
        if self.state != HudState::Paused || self.mode == HudMode::Listening {
            ctx.request_repaint();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let available = ui.available_rect_before_wrap();

                // The content rect, inset from window edges to leave room for glow
                let glow_margin = GLOW_SPREAD * GLOW_LAYERS as f32;
                let panel_rect = available.shrink(glow_margin);

                let painter = ui.painter();

                // Paint the glow border
                self.paint_glow(painter, panel_rect, time_s);

                // Paint the solid background
                painter.rect_filled(panel_rect, CORNER_RADIUS, self.bg_color(dark));

                // Paint a subtle border for paused state
                if self.state == HudState::Paused {
                    let border_color = if dark {
                        Color32::from_rgb(60, 60, 70)
                    } else {
                        Color32::from_rgb(210, 210, 215)
                    };
                    painter.rect_stroke(
                        panel_rect,
                        CORNER_RADIUS,
                        Stroke::new(1.0, border_color),
                    );
                }

                // Content area inside the panel
                let content_rect = panel_rect.shrink(PADDING);
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                    match self.mode {
                        HudMode::Listening => {
                            if self.is_empty() {
                                let placeholder = match self.state {
                                    HudState::Loading => "Starting...",
                                    HudState::Recording => "Speak now...",
                                    HudState::Paused => "Paused",
                                };
                                ui.label(
                                    egui::RichText::new(placeholder)
                                        .font(FontId::new(FONT_SIZE, FontFamily::Proportional))
                                        .color(self.placeholder_color(dark)),
                                );
                            } else {
                                let layout = self.build_text_layout(dark, elapsed_ms);
                                let response = ui.label(layout);

                                // Click on text to enter edit mode
                                if response.clicked() && self.state == HudState::Recording {
                                    self.edit_buffer = self.full_text();
                                    self.stop_listening();
                                    self.mode = HudMode::Editing;
                                }
                            }
                        }
                        HudMode::Editing => {
                            let text_col = self.text_color(dark);
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut self.edit_buffer)
                                    .desired_width(f32::INFINITY)
                                    .font(FontId::new(FONT_SIZE, FontFamily::Proportional))
                                    .frame(false)
                                    .text_color(text_col),
                            );

                            // Auto-focus the text editor
                            if response.gained_focus() || !response.has_focus() {
                                response.request_focus();
                            }

                            // Hint at bottom
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Cmd+Enter submit  •  Esc cancel")
                                    .font(FontId::new(12.0, FontFamily::Proportional))
                                    .color(if dark { DARK_DIM } else { LIGHT_DIM }),
                            );
                        }
                    }
                });

                // Window dragging — drag from any empty area
                let response = ui.interact(
                    panel_rect,
                    ui.id().with("drag"),
                    egui::Sense::drag(),
                );
                if response.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

pub fn run_ui() -> anyhow::Result<()> {
    let final_text = Arc::new(Mutex::new(None::<String>));
    let final_text_clone = Arc::clone(&final_text);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(
                WINDOW_WIDTH + GLOW_SPREAD * GLOW_LAYERS as f32 * 2.0,
                WINDOW_MIN_HEIGHT + GLOW_SPREAD * GLOW_LAYERS as f32 * 2.0,
            ))
            .with_min_inner_size(Vec2::new(300.0, WINDOW_MIN_HEIGHT))
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(true),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Claudio",
        options,
        Box::new(move |_cc| {
            let mut app = HudApp::new(final_text_clone);
            if let Err(e) = app.start_listening() {
                eprintln!("Failed to start speech recognition: {}", e);
                eprintln!(
                    "Make sure you have granted microphone and speech recognition permissions."
                );
                std::process::exit(1);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Output the final text to stdout, same as terminal mode
    if let Some(text) = final_text.lock().unwrap().take() {
        if !text.is_empty() {
            println!("{}", text);
        }
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}
