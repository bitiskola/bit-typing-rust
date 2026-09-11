//! egui interface for bit-typing, rewritten 1-to-1 from the SwiftUI version.
//!
//! Layout mirrors `ContentView` + `LessonView` / `StatsView` / `EditorView` /
//! `ResultsView` / `OptionsView` / `SetupView` / `AboutView` in the Swift app:
//! * native-style sidebar (3 rows, blue selection) + per-page toolbar;
//! * open lesson page: scrolling text, virtual keyboard, progress footer —
//!   no cards or hairlines, monochrome with a single blue accent;
//! * sheets for options / setup / about / results + "Well done!" overlay.
//!
//! The typing engine (`session`), persistence and import logic are unchanged.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use egui::{Align, Color32, Layout, RichText};

use crate::audio::{self, ClickPlayer};
use crate::courses::{
    self, BUILTIN_COURSE_FILES, load_custom_courses, next_lesson, read_course_text,
    sanitise_lesson_name,
};
use crate::history::{Attempt, load_history, save_history};
use crate::i18n::I18n;
use crate::keyboards::{KeyboardLayout, load_layouts};
use crate::paths;
use crate::session::{CharacterOutcome, Clock, Session, SystemClock, Tip, overall_score};
use crate::settings::{Settings, TYPO_MODES};
use crate::theme::{self, FingerZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Lesson,
    Stats,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultsTab {
    Overview,
    Details,
    Errors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartMetric {
    Speed,
    AccuracyChar,
    AccuracyWord,
}

pub struct BitTypingApp {
    root: PathBuf,
    settings: Settings,
    i18n: I18n,
    layouts: BTreeMap<String, KeyboardLayout>,
    course_paths: Vec<PathBuf>,
    custom: BTreeSet<String>,
    history: Vec<Attempt>,
    session: Session,
    current_lesson: String,
    last_lesson_name: String,
    page: Page,
    sidebar_collapsed: bool,
    // Lesson text glide: the pinned shift eases toward its target for
    // ~0.12s after every character switch (Swift `withAnimation`).
    text_shift: f32,
    text_anim_from: f32,
    text_anim_t0: Instant,
    text_animating: bool,
    // Overlays
    show_options: bool,
    show_about: bool,
    show_setup: bool,
    results: Option<Attempt>,
    results_tab: ResultsTab,
    chart_metric: ChartMetric,
    well_done_until: Option<Instant>,
    pending_results: Option<Attempt>,
    // Editor
    editor_name: String,
    editor_text: String,
    stats_selected: Option<usize>,
    confirm_clear_history: bool,
    // Options form (strings for text fields)
    opt_language: String,
    opt_keyboard: String,
    opt_typo: String,
    opt_goal_wpm: String,
    opt_goal_accuracy: String,
    opt_timeout: String,
    opt_timed: String,
    opt_backspace: bool,
    opt_auto_start: bool,
    opt_sound: bool,
    opt_metronome: bool,
    opt_error: String,
    // Setup shares the options form.
    confirm_replace: Option<PathBuf>,
    pending_imports: Vec<PathBuf>,
    // Misc
    audio: ClickPlayer,
    clock: SystemClock,
    save_error: String,
    // Retained brand/avatar textures (None => styled initials placeholder).
    brand_icon: Option<egui::TextureHandle>,
    avatar_ms: Option<egui::TextureHandle>,
    avatar_cacto: Option<egui::TextureHandle>,
}

impl BitTypingApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let root = paths::runtime_root();
        paths::ensure_dirs(&root);
        audio::ensure_sounds(&paths::sounds_dir(&root));

        let settings = Settings::load(&paths::settings_file(&root));
        let i18n = I18n::load(&paths::langs_dir(&root));
        let layouts = load_layouts(&paths::keyboards_dir(&root));
        let course_paths = courses::list_courses(&paths::courses_dir(&root));
        let custom = load_custom_courses(&paths::custom_courses_file(&root));
        let history = load_history(&paths::history_file(&root));

        let current_lesson = course_paths
            .first()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let text = current_lesson
            .is_empty()
            .then(String::new)
            .unwrap_or_else(|| {
                lesson_text(&root, &course_paths, &current_lesson, "Choose a lesson")
            });
        let mut session = Session::new(&current_lesson, &text);
        session.tip = Tip::Initial;

        let audio = ClickPlayer::new(&paths::sounds_dir(&root));
        audio.set_enabled(settings.sound);

        // Brand + profile pictures, bundled via rust-embed (`assets/*.png`).
        // Missing/invalid files fall back to styled initials placeholders.
        let brand_icon = load_bundled_texture(&cc.egui_ctx, "about-brand-icon", "assets/favico.png", 192);
        let avatar_ms = load_bundled_texture(
            &cc.egui_ctx,
            "about-avatar-ms",
            "assets/micr0softstore.png",
            192,
        );
        let avatar_cacto = load_bundled_texture(
            &cc.egui_ctx,
            "about-avatar-cacto",
            "assets/cacto.tsx.png",
            192,
        );

        let show_setup = !settings.setup_complete;
        let mut app = Self {
            root,
            opt_language: settings.language.clone(),
            opt_keyboard: settings.keyboard_layout.clone(),
            opt_typo: settings.typo_mode.clone(),
            opt_goal_wpm: settings.goal_wpm.to_string(),
            opt_goal_accuracy: settings.goal_accuracy.to_string(),
            opt_timeout: settings.timeout_seconds.to_string(),
            opt_timed: settings.timed_minutes.to_string(),
            opt_backspace: settings.backspace,
            opt_auto_start: settings.auto_start,
            opt_sound: settings.sound,
            opt_metronome: settings.metronome,
            opt_error: String::new(),
            settings,
            i18n,
            layouts,
            course_paths,
            custom,
            history,
            session,
            current_lesson: current_lesson.clone(),
            last_lesson_name: current_lesson,
            page: Page::Lesson,
            sidebar_collapsed: false,
            text_shift: 0.0,
            text_anim_from: 0.0,
            text_anim_t0: Instant::now(),
            text_animating: false,
            show_options: false,
            show_about: false,
            show_setup,
            results: None,
            results_tab: ResultsTab::Overview,
            chart_metric: ChartMetric::Speed,
            well_done_until: None,
            pending_results: None,
            editor_name: String::new(),
            editor_text: String::new(),
            stats_selected: None,
            confirm_clear_history: false,
            confirm_replace: None,
            pending_imports: Vec::new(),
            audio,
            clock: SystemClock::default(),
            save_error: String::new(),
            brand_icon,
            avatar_ms,
            avatar_cacto,
        };
        if app.show_setup {
            app.sync_options_form();
        }
        app
    }

    fn t(&self, key: &str) -> String {
        self.i18n.t(&self.settings.language, key)
    }

    fn dark(&self, ctx: &egui::Context) -> bool {
        ctx.style().visuals.dark_mode
    }

    fn save_settings(&self) {
        let _ = self.settings.save(&paths::settings_file(&self.root));
    }

    // --- Speed unit (Swift `SpeedUnit`, shared through settings) ---

    fn is_cpm(&self) -> bool {
        self.settings.speed_unit == "cpm"
    }

    fn speed_factor(&self) -> f64 {
        if self.is_cpm() { 5.0 } else { 1.0 }
    }

    fn speed_unit_key(&self) -> &'static str {
        if self.is_cpm() { "cpm_unit" } else { "wpm_unit" }
    }

    fn format_speed(&self, wpm: f64) -> String {
        format!("{:.1}", wpm * self.speed_factor())
    }

    fn set_speed_unit(&mut self, cpm: bool) {
        self.settings.speed_unit = if cpm { "cpm".to_string() } else { "wpm".to_string() };
        self.save_settings();
    }

    fn history_summary(&self) -> String {
        if self.history.is_empty() {
            return self.t("no_attempts");
        }
        let best = self.history.iter().map(|a| a.wpm).fold(0.0, f64::max);
        let avg =
            self.history.iter().map(|a| a.accuracy).sum::<f64>() / self.history.len() as f64;
        self.i18n.history_summary(
            &self.settings.language,
            self.history.len(),
            best * self.speed_factor(),
            &self.t(self.speed_unit_key()),
            avg,
        )
    }

    fn reload_courses(&mut self, select: Option<&str>) {
        self.course_paths = courses::list_courses(&paths::courses_dir(&self.root));
        let names: Vec<String> = self
            .course_paths
            .iter()
            .map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string())
            .collect();
        if let Some(sel) = select {
            if names.contains(&sel.to_string()) {
                self.current_lesson = sel.to_string();
            }
        }
        if !names.contains(&self.current_lesson) {
            self.current_lesson = names.first().cloned().unwrap_or_default();
        }
        self.prepare_lesson();
    }

    fn current_path(&self) -> Option<PathBuf> {
        self.course_paths.iter().find(|p| {
            p.file_stem().map(|s| s.to_string_lossy().to_string()) == Some(self.current_lesson.clone())
        }).cloned()
    }

    fn prepare_lesson(&mut self) {
        let path = self.current_path();
        let fallback = self.t("initial_tip");
        let text = match &path {
            Some(p) => match read_course_text(p) {
                Ok(t) => t.trim_end_matches(['\r', '\n']).to_string(),
                Err(e) => {
                    self.save_error =
                        format!("{}: {e}", self.t("lesson_load_failed_title"));
                    String::new()
                }
            },
            None => fallback.clone(),
        };
        if let Some(p) = &path {
            self.last_lesson_name = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
        }
        self.session = Session::new(&self.current_lesson, &text);
        self.session.tip = Tip::Ready;
    }

    fn start_session(&mut self) {
        self.session.start(&self.clock);
    }

    fn toggle_start(&mut self) {
        if self.current_path().is_none() {
            return;
        }
        self.session.toggle(&self.clock);
    }

    fn play(&self, correct: bool) {
        if self.settings.sound {
            self.audio.play(correct);
        }
    }

    /// Freeze the current glide origin: the next frames ease the pinned
    /// shift from here to the new character (called on every switch).
    fn begin_text_anim(&mut self) {
        self.text_anim_from = self.text_shift;
        self.text_anim_t0 = Instant::now();
        self.text_animating = true;
    }

    fn finish_session(&mut self) {
        if !self.session.running {
            return;
        }
        self.session.running = false;
        let path = self.current_path();
        let course_file = path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let custom = path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| self.custom.contains(&n.to_string_lossy().to_string()))
            .unwrap_or(false);
        let attempt =
            self.session
                .finish(&self.settings, &self.clock, &course_file, custom);
        self.history.push(attempt.clone());
        let _ = save_history(&paths::history_file(&self.root), &self.history);
        self.well_done_until = Some(Instant::now() + std::time::Duration::from_secs(3));
        self.pending_results = Some(attempt);
    }

    fn sync_options_form(&mut self) {
        self.opt_language = self.settings.language.clone();
        self.opt_keyboard = self.settings.keyboard_layout.clone();
        self.opt_typo = self.settings.typo_mode.clone();
        self.opt_goal_wpm = trim_num(self.settings.goal_wpm);
        self.opt_goal_accuracy = trim_num(self.settings.goal_accuracy);
        self.opt_timeout = trim_num(self.settings.timeout_seconds);
        self.opt_timed = trim_num(self.settings.timed_minutes);
        self.opt_backspace = self.settings.backspace;
        self.opt_auto_start = self.settings.auto_start;
        self.opt_sound = self.settings.sound;
        self.opt_metronome = self.settings.metronome;
        self.opt_error.clear();
    }

    /// Validate + apply the options form. Returns success.
    fn apply_options_form(&mut self) -> bool {
        let goal_wpm: f64 = match self.opt_goal_wpm.trim().parse::<f64>() {
            Ok(v) => v.max(1.0),
            Err(_) => return self.options_invalid(),
        };
        let goal_accuracy: f64 = match self.opt_goal_accuracy.trim().parse::<f64>() {
            Ok(v) => v.clamp(0.0, 100.0),
            Err(_) => return self.options_invalid(),
        };
        let timeout: f64 = match self.opt_timeout.trim().parse::<f64>() {
            Ok(v) => v.max(0.1),
            Err(_) => return self.options_invalid(),
        };
        let timed: f64 = match self.opt_timed.trim().parse::<f64>() {
            Ok(v) => v.max(0.0),
            Err(_) => return self.options_invalid(),
        };
        if !TYPO_MODES.contains(&self.opt_typo.as_str()) {
            return self.options_invalid();
        }
        self.settings.language = self.opt_language.clone();
        self.settings.keyboard_layout = self.opt_keyboard.clone();
        self.settings.typo_mode = self.opt_typo.clone();
        self.settings.goal_wpm = goal_wpm;
        self.settings.goal_accuracy = goal_accuracy;
        self.settings.timeout_seconds = timeout;
        self.settings.timed_minutes = timed;
        self.settings.backspace = self.opt_backspace;
        self.settings.auto_start = self.opt_auto_start;
        self.settings.sound = self.opt_sound;
        self.settings.metronome = self.opt_metronome;
        self.audio.set_enabled(self.settings.sound);
        self.save_settings();
        // Re-translate dynamic UI (lesson tip etc.).
        self.prepare_lesson();
        true
    }

    fn options_invalid(&mut self) -> bool {
        self.opt_error = format!(
            "{}: {}",
            self.t("invalid_options_title"),
            self.t("invalid_options_body")
        );
        false
    }

    /// The lesson owns every keystroke: a focused button turns Space/Enter
    /// into a fake click (egui `FAKE_PRIMARY_CLICKED`), so clicking Start
    /// would pause the lesson again at the first space instead of typing
    /// it. Drop such focus every frame; text fields keep theirs.
    fn drop_stolen_focus(&self, ctx: &egui::Context) {
        if self.page != Page::Lesson
            || self.results.is_some()
            || self.show_options
            || self.show_setup
            || self.show_about
            || self.well_done_until.is_some()
        {
            return;
        }
        // NOTE: no `wants_keyboard_input` bail-out here on purpose: on the
        // lesson page no text field exists, and that check is true for ANY
        // focused widget (e.g. the Start button), which would freeze both
        // typing and this guard forever. Modals are already excluded above.
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }

    /// Global key handling. Called once per frame with copied events to keep
    /// borrow rules simple. Guards mirror Swift `AppState.handle`.
    fn handle_input(&mut self, ctx: &egui::Context) {
        enum Action {
            Text(String),
            Backspace,
            Escape,
            Restart,
            Enter,
        }
        let mut actions: Vec<Action> = Vec::new();
        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(egui::Key::R) {
                actions.push(Action::Restart);
            }
            for event in &i.events {
                match event {
                    egui::Event::Text(text) => {
                        // egui also emits Text for shortcuts; ignore control combos.
                        if !i.modifiers.ctrl && !i.modifiers.command {
                            actions.push(Action::Text(text.clone()));
                        }
                    }
                    egui::Event::Key { key, pressed: true, .. } => match key {
                        egui::Key::Backspace => actions.push(Action::Backspace),
                        egui::Key::Escape => actions.push(Action::Escape),
                        egui::Key::Enter => actions.push(Action::Enter),
                        _ => {}
                    },
                    _ => {}
                }
            }
        });
        if actions.is_empty() {
            return;
        }
        // Guards mirror the Swift version: no typing into sheets / other tabs.
        if self.results.is_some()
            || self.show_options
            || self.show_setup
            || self.show_about
            || self.well_done_until.is_some()
            || self.page != Page::Lesson
        {
            return;
        }
        // NOTE: no `wants_keyboard_input` bail-out here on purpose: it is
        // true for ANY focused widget (e.g. the Start button after clicking
        // it), which froze all typing until restart. Sheets and other pages
        // are already excluded above, and the lesson page owns no text
        // field — `drop_stolen_focus` additionally clears button focus so
        // Space/Enter can never turn into fake button clicks mid-lesson.
        for action in actions {
            match action {
                Action::Escape => {
                    if self.session.running {
                        self.toggle_start();
                    }
                }
                Action::Restart => self.prepare_lesson(),
                Action::Backspace => {
                    if self.session.running && !self.session.paused {
                        let before = self.session.index;
                        self.session.handle_backspace(&self.settings, &self.clock);
                        if self.session.index != before {
                            self.begin_text_anim();
                        }
                        self.play(true);
                    }
                }
                Action::Enter => self.handle_char_input('\n'),
                Action::Text(text) => {
                    for ch in text.chars() {
                        if ch == '\r' {
                            self.handle_char_input('\n');
                        } else if ch == '\n' || ch == '\t' || !ch.is_control() {
                            self.handle_char_input(ch);
                        }
                    }
                }
            }
        }
    }

    fn handle_char_input(&mut self, actual: char) {
        if self.session.index >= self.session.text.len() {
            return;
        }
        if !self.session.running {
            if self.settings.auto_start && self.current_path().is_some() {
                self.start_session();
            } else {
                return;
            }
        }
        if self.session.paused {
            return;
        }
        let before = self.session.index;
        let outcome = self.session.handle_character(&self.settings, &self.clock, actual);
        match outcome {
            CharacterOutcome::Accepted { correct } => self.play(correct),
            CharacterOutcome::Blocked => self.play(false),
            CharacterOutcome::Finished => {
                self.play(true);
                self.finish_session();
            }
            CharacterOutcome::Ignored => {}
        }
        if self.session.index != before {
            self.begin_text_anim();
        }
    }
}

fn lesson_text(_root: &Path, paths: &[PathBuf], stem: &str, fallback: &str) -> String {
    paths
        .iter()
        .find(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()) == Some(stem.to_string()))
        .and_then(|p| read_course_text(p).ok())
        .map(|t| t.trim_end_matches(['\r', '\n']).to_string())
        .unwrap_or_else(|| fallback.to_string())
}

fn trim_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        v.to_string()
    }
}

/// Load a bundled PNG (`assets/…`, embedded via rust-embed) as a retained
/// egui texture, downscaled to `max_px` for a lean upload. Returns `None`
/// when the file is missing or undecodable — callers then show a styled
/// initials placeholder instead (per `assets/README.md`).
fn load_bundled_texture(
    ctx: &egui::Context,
    name: &str,
    key: &str,
    max_px: u32,
) -> Option<egui::TextureHandle> {
    let file = crate::EmbeddedAssets::get(key)?;
    let img = image::load_from_memory(&file.data).ok()?;
    let img = img.thumbnail(max_px, max_px).to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &img.into_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
        }
    }
}

// ---------------------------------------------------------------------------
// eframe::App
// ---------------------------------------------------------------------------

impl eframe::App for BitTypingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep the Swift monochrome theme applied (adaptive to OS appearance).
        theme::apply(ctx);
        // Well-done splash expiry.
        if let Some(until) = self.well_done_until {
            if Instant::now() >= until {
                self.well_done_until = None;
                if let Some(attempt) = self.pending_results.take() {
                    self.results_tab = ResultsTab::Overview;
                    self.chart_metric = ChartMetric::Speed;
                    self.results = Some(attempt);
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        // Live metrics + timed-lesson cutoff.
        if self.session.running && !self.session.paused {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            let limit = self.settings.timed_minutes * 60.0;
            if limit > 0.0 && self.session.elapsed(self.clock.now()) >= limit {
                self.finish_session();
            }
        }

        self.drop_stolen_focus(ctx);
        self.handle_input(ctx);
        // Drag & drop import (drop .txt files anywhere).
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            self.pending_imports = dropped;
            self.import_pending(false);
        }

        self.toolbar(ctx);
        self.side_bar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| match self.page {
            Page::Lesson => self.lesson_page(ui),
            Page::Stats => self.stats_page(ui),
            Page::Editor => self.editor_page(ui),
        });

        if self.show_setup {
            self.setup_window(ctx);
        } else if self.show_options {
            self.options_window(ctx);
        }
        if self.show_about {
            self.about_window(ctx);
        }
        if self.well_done_until.is_some() {
            self.well_done_overlay(ctx);
        }
        if self.results.is_some() {
            self.results_window(ctx);
        }
        if self.confirm_clear_history {
            self.clear_history_dialog(ctx);
        }
        if self.confirm_replace.is_some() {
            self.replace_dialog(ctx);
        }
        if !self.save_error.is_empty() {
            let mut clear = false;
            let dark = self.dark(ctx);
            egui::Window::new(self.t("lesson_load_failed_title"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(RichText::new(&self.save_error).color(theme::error(dark)));
                    if ui
                        .add(theme::secondary_button(RichText::new("OK"), dark))
                        .clicked()
                    {
                        clear = true;
                    }
                });
            if clear {
                self.save_error.clear();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Chrome: toolbar + sidebar (Swift `ContentView` + per-page `.toolbar`)
// ---------------------------------------------------------------------------

impl BitTypingApp {
    /// Per-page toolbar. The leading sidebar toggle mirrors Swift's
    /// toolbar toggle; the lesson page packs picker pill + blue circle +
    /// outlined stats pill left-aligned, exactly like the Swift toolbar.
    fn toolbar(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        let page = self.page;
        // Fixed height on every page: the sidebar below would otherwise grow
        // and shrink with the tallest toolbar control.
        egui::TopBottomPanel::top("toolbar")
            .show_separator_line(false)
            .exact_height(52.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    // Sidebar collapse toggle (Swift toolbar toggle).
                    let bar_entry = ui.available_width();
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("☰").size(15.0).color(theme::ink(dark)),
                            )
                            .fill(Color32::TRANSPARENT)
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(36.0, 32.0)),
                        )
                        .clicked()
                    {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                    }
                    ui.add_space(8.0);
                    let consumed = bar_entry - ui.available_width();
                    match page {
                        Page::Lesson => self.lesson_toolbar(ui, dark, consumed),
                        Page::Stats => self.stats_toolbar(ui, dark),
                        Page::Editor => self.editor_toolbar(ui, dark),
                    }
                });
            });
    }

    fn lesson_toolbar(&mut self, ui: &mut egui::Ui, dark: bool, consumed: f32) {
        // `consumed` = exact width the toolbar used before us (toggle row).
        let entry_avail = ui.available_width();
        // Lesson picker pill (navigation placement in Swift).
        let names: Vec<String> = self
            .course_paths
            .iter()
            .map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string())
            .collect();
        if names.is_empty() {
            ui.label(
                RichText::new(self.t("no_lessons"))
                    .size(13.0)
                    .color(theme::muted(dark)),
            );
        } else {
            // Dark rounded pill like Swift's picker (radius 16).
            ui.scope(|ui| {
                let visuals = &mut ui.style_mut().visuals.widgets;
                visuals.inactive.bg_fill = theme::panel(dark);
                visuals.inactive.corner_radius = egui::CornerRadius::same(16);
                visuals.hovered.bg_fill = theme::panel2(dark);
                visuals.hovered.corner_radius = egui::CornerRadius::same(16);
                visuals.open.bg_fill = theme::panel2(dark);
                visuals.open.corner_radius = egui::CornerRadius::same(16);
                let selected = self.current_lesson.clone();
                egui::ComboBox::from_id_salt("lesson_combo")
                    .selected_text(format!("{selected}  ↕"))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for name in &names {
                            if ui
                                .selectable_value(&mut self.current_lesson, name.clone(), name)
                                .clicked()
                            {
                                self.prepare_lesson();
                            }
                        }
                    });
            });
        }
        ui.add_space(8.0);
        // Blue circle Start/Pause (Swift prominent action); the label lives
        // in the hover tooltip since the circle is icon-only.
        let (glyph, key) = if !self.session.running {
            ("▶", "start_lesson")
        } else if self.session.paused {
            ("▶", "resume")
        } else {
            ("⏸", "pause")
        };
        let hover = self.t(key);
        let enabled = self.current_path().is_some();
        ui.add_enabled_ui(enabled, |ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(glyph).size(16.0).color(Color32::WHITE),
                    )
                    .fill(theme::brand(dark))
                    .corner_radius(egui::CornerRadius::same(22))
                    .min_size(egui::vec2(44.0, 44.0)),
                )
                .on_hover_text(hover)
                .clicked()
            {
                self.toggle_start();
            }
        });
        ui.add_space(8.0);
        // Outlined stats pill, centered in the whole bar: the live strings
        // are measured every frame so the pill stays pixel-centered as the
        // numbers change width (never drifts, never overlaps the circle).
        let now = self.clock.now();
        let m = self.session.live_metrics(now);
        let total = m.elapsed as u64;
        let time_v = format!("{}:{:02}", total / 60, total % 60);
        let acc_v = format!("{:.0}%", m.accuracy);
        let spd_v = format!(
            "{} {}",
            (m.wpm * self.speed_factor()).round() as i64,
            self.t(self.speed_unit_key())
        );
        let time_k = self.t("time");
        let acc_k = self.t("accuracy");
        let spd_k = self.t("speed");
        let mut pill_w = 0.0_f32;
        for (title, value) in [(&time_k, &time_v), (&acc_k, &acc_v), (&spd_k, &spd_v)] {
            pill_w += text_w(ui, 11.0, title) + 6.0 + text_w(ui, 15.0, value);
        }
        pill_w += 18.0 * 2.0 + 28.0; // pair gaps + frame side margins
        let panel_w = entry_avail + consumed;
        let cursor = consumed + (entry_avail - ui.available_width());
        let pad = (panel_w / 2.0 - cursor - pill_w / 2.0).max(0.0);
        ui.add_space(pad);
        egui::Frame::new()
            .fill(theme::panel(dark))
            .stroke(egui::Stroke::new(1.0_f32, theme::border(dark)))
            .corner_radius(egui::CornerRadius::same(16))
            .inner_margin(egui::Margin::symmetric(14, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    metric_view(ui, &time_k, &time_v, dark);
                    metric_view(ui, &acc_k, &acc_v, dark);
                    metric_view(ui, &spd_k, &spd_v, dark);
                });
            });
    }

    fn stats_toolbar(&mut self, ui: &mut egui::Ui, dark: bool) {
        // WPM / CPM segmented pill (Swift `Picker(.segmented)`, width 200).
        speed_segmented(ui, self.is_cpm(), &self.t("wpm_unit"), &self.t("cpm_unit"), dark, |cpm| {
            self.set_speed_unit(cpm);
        });
        ui.add_space(4.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_clear = !self.history.is_empty();
            if ui
                .add_enabled(
                    can_clear,
                    theme::secondary_button(
                        RichText::new(format!("🗑 {}", self.t("clear_history"))).size(13.0),
                        dark,
                    ),
                )
                .clicked()
            {
                self.confirm_clear_history = true;
            }
            ui.add_space(4.0);
            let can_review = self.stats_selected.is_some();
            if ui
                .add_enabled(
                    can_review,
                    theme::secondary_button(
                        RichText::new(format!("🔍 {}", self.t("review_selected"))).size(13.0),
                        dark,
                    ),
                )
                .clicked()
            {
                if let Some(idx) = self.stats_selected {
                    if let Some(attempt) = self.history.get(idx).cloned() {
                        self.results_tab = ResultsTab::Overview;
                        self.chart_metric = ChartMetric::Speed;
                        self.results = Some(attempt);
                    }
                }
            }
        });
    }

    fn editor_toolbar(&mut self, ui: &mut egui::Ui, dark: bool) {
        if ui
            .add(theme::secondary_button(RichText::new("+").size(15.0), dark).min_size(egui::vec2(44.0, 32.0)))
            .on_hover_text(self.t("new"))
            .clicked()
        {
            self.editor_name.clear();
            self.editor_text.clear();
        }
        if ui
            .add(theme::secondary_button(RichText::new("📥").size(14.0), dark).min_size(egui::vec2(44.0, 32.0)))
            .on_hover_text(self.t("import_custom_lesson"))
            .clicked()
        {
            // Native file picker (Ubuntu Files / Finder / Explorer).
            if let Some(files) = rfd::FileDialog::new()
                .add_filter(&format!("{} (.txt)", self.t("text_lessons")), &["txt"])
                .add_filter(self.t("all_files"), &["*"])
                .set_title(self.t("import_text_lessons"))
                .pick_files()
            {
                self.pending_imports = files;
                self.import_pending(true);
            }
        }
        if ui
            .add(theme::secondary_button(RichText::new("📁").size(13.0), dark).min_size(egui::vec2(44.0, 32.0)))
            .on_hover_text(self.t("load_selected"))
            .clicked()
        {
            self.load_editor_from_current();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(
                    theme::brand_button(
                        RichText::new(format!("✔ {}", self.t("save_lesson"))).size(13.0),
                        dark,
                    )
                    .min_size(egui::vec2(120.0, 32.0)),
                )
                .clicked()
            {
                self.save_editor_lesson();
            }
        });
    }

    /// Sidebar: inset rounded panel with a hairline border (Swift sidebar),
    /// plain rows with a blue pill for the selection. Collapses via the
    /// toolbar toggle, like Swift's `NavigationSplitView`.
    fn side_bar(&mut self, ctx: &egui::Context) {
        if self.sidebar_collapsed {
            return;
        }
        let dark = self.dark(ctx);
        egui::SidePanel::left("side_bar")
            .resizable(false)
            .default_width(204.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::background(dark))
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                theme::card_frame(dark).show(ui, |ui| {
                    ui.set_min_height(ui.available_height());
                    ui.add_space(8.0);
                    let items = [
                        (Page::Lesson, "⌨", self.t("current_lesson")),
                        (Page::Stats, "📊", self.t("student_statistics")),
                        (Page::Editor, "📝", self.t("lesson_editor")),
                    ];
                    for (page, icon, label) in items {
                        let active = self.page == page;
                        let fill = if active {
                            theme::brand(dark)
                        } else {
                            Color32::TRANSPARENT
                        };
                        let text = if active {
                            Color32::WHITE
                        } else {
                            theme::ink(dark)
                        };
                        let btn = egui::Button::new(
                            RichText::new(format!("{icon}  {label}")).size(13.0).color(text),
                        )
                        .fill(fill)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(ui.available_width(), 34.0));
                        // Left-align the label inside the full-width row.
                        if ui
                            .with_layout(Layout::top_down(Align::Min), |ui| ui.add(btn))
                            .inner
                            .clicked()
                        {
                            self.page = page;
                            if page == Page::Editor {
                                self.load_editor_from_current();
                            }
                        }
                        ui.add_space(2.0);
                    }
                    // Bottom: Options + About. (Swift keeps these in the app
                    // menu, which doesn't exist on Linux, so they live here
                    // instead.)
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        ui.add_space(8.0);
                        if ui
                            .add(
                                theme::secondary_button(
                                    RichText::new(self.t("about")).size(13.0),
                                    dark,
                                )
                                .min_size(egui::vec2(ui.available_width(), 32.0)),
                            )
                            .clicked()
                        {
                            self.show_about = true;
                        }
                        ui.add_space(4.0);
                        if ui
                            .add(
                            theme::secondary_button(
                                RichText::new(self.t("options")).size(13.0),
                                dark,
                            )
                                .min_size(egui::vec2(ui.available_width(), 32.0)),
                            )
                            .clicked()
                        {
                            self.sync_options_form();
                            self.show_options = true;
                        }
                    });
                });
            });
    }
}

/// Small `TIME 0:00 / ACCURACY 100% / SPEED` readout: label beside the value
/// (not above) so the trio breathes. Mirrors Swift `MetricView`.
fn metric_view(ui: &mut egui::Ui, title: &str, value: &str, dark: bool) {    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(title)
                .size(11.0)
                .strong()
                .color(theme::muted(dark)),
        );
        ui.label(
            RichText::new(value)
                .size(15.0)
                .strong()
                .color(theme::ink(dark)),
        );
    });
}

/// Measured width of a single-line proportional label, for centering math
/// (same metrics as displayed: advances don't depend on color/weight).
fn text_w(ui: &egui::Ui, size: f32, s: &str) -> f32 {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        s,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            ..Default::default()
        },
    );
    ui.ctx().fonts(|f| f.layout_job(job).size().x)
}

/// WPM / CPM segmented capsule (Swift `Picker(.segmented)`, width 200).
fn speed_segmented(
    ui: &mut egui::Ui,
    is_cpm: bool,
    wpm_label: &str,
    cpm_label: &str,
    dark: bool,
    mut on_pick: impl FnMut(bool),
) {
    egui::Frame::new()
        .fill(theme::panel2(dark))
        .corner_radius(egui::CornerRadius::same(16))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (cpm, label) in [(false, wpm_label), (true, cpm_label)] {
                    let selected = is_cpm == cpm;
                    let btn = if selected {
                        theme::primary_button(
                            RichText::new(label).size(12.0).strong(),
                            dark,
                        )
                        .min_size(egui::vec2(88.0, 26.0))
                    } else {
                        egui::Button::new(
                            RichText::new(label).size(12.0).color(theme::muted(dark)),
                        )
                        .fill(Color32::TRANSPARENT)
                        .corner_radius(egui::CornerRadius::same(13))
                        .min_size(egui::vec2(88.0, 26.0))
                    };
                    if ui.add(btn).clicked() {
                        on_pick(cpm);
                    }
                }
            });
        });
}

// ---------------------------------------------------------------------------
// Lesson page (Swift `LessonView`: open text, keyboard, progress footer)
// ---------------------------------------------------------------------------

impl BitTypingApp {
    fn lesson_page(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark(ui.ctx());
        // Swift LessonView: VStack(spacing: 24) with h20/v18 padding.
        // Every block has an explicit height measured from the live panel
        // size, so fullscreen distributes exactly like Swift: the open text
        // area absorbs the remaining space (min 170), keyboard is 300,
        // footer is 30 — no voids, no overflow, at any window size.
        const PAD_X: f32 = 20.0;
        const PAD_Y: f32 = 18.0;
        const GAP: f32 = 24.0;
        const KB_H: f32 = 300.0;
        const FOOT_H: f32 = 30.0;
        let top_w = ui.available_width();
        let top_h = ui.available_height();
        let column_w = (top_w - 2.0 * PAD_X).max(100.0);
        let text_h = (top_h - 2.0 * PAD_Y - GAP - KB_H - GAP - FOOT_H).clamp(170.0, 4000.0);
        let left_pad = ((top_w - column_w) / 2.0).max(0.0);
        ui.add_space(PAD_Y);
        ui.horizontal(|ui| {
            ui.add_space(left_pad);
            ui.allocate_ui_with_layout(
                egui::vec2(column_w, text_h + GAP + KB_H + GAP + FOOT_H),
                Layout::top_down(Align::Min),
                |ui| {
                    self.lesson_text_pinned(ui, dark, column_w, text_h);
                    ui.add_space(GAP);
                    self.keyboard_block(ui, dark);
                    ui.add_space(GAP);
                    self.lesson_footer(ui, dark);
                },
            );
        });
        ui.add_space(PAD_Y);
    }

    /// Non-scrollable lesson viewport, always focused on the current
    /// character: the row is pinned so the active character sits exactly in
    /// the middle (Swift `scrollTo(anchor: .center)`), clipped to the
    /// region. No scrollbars, no scroll state — the windowed text
    /// (~80 finished left / ~160 upcoming right, 44pt mono) re-centers on
    /// every keystroke by construction.
    fn lesson_text_pinned(&mut self, ui: &mut egui::Ui, dark: bool, width: f32, height: f32) {
        let (region, _) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let chars: Vec<char> = self.session.text.iter().copied().collect();
        if chars.is_empty() {
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(region)
                    .layout(Layout::top_down(Align::Center)),
            )
            .centered_and_justified(|ui| {
                ui.label(
                    RichText::new(self.t("no_lessons"))
                        .size(13.0)
                        .color(theme::muted(dark)),
                );
            });
            return;
        }
        // Swift `visibleOffsets`: ~80 left, ~160 right, max 240 cells.
        let count = chars.len();
        let cursor = self.session.index.min(count - 1);
        let low = cursor.saturating_sub(80);
        let high = count.min(low + 240).min(cursor + 160);
        let active = self.session.index;
        let has_active = active < high && active < count;

        let mono44 = egui::TextFormat {
            font_id: egui::FontId::monospace(44.0),
            color: theme::pending(dark),
            ..Default::default()
        };
        let mut before = egui::text::LayoutJob::default();
        for i in low..active.min(high) {
            let ch = chars[i];
            let state = self
                .session
                .states
                .get(&i)
                .map(|s| s.as_str())
                .unwrap_or("correct");
            before.append(
                &display_char(ch),
                0.0,
                egui::TextFormat {
                    color: theme::state_color_dark(state, dark),
                    ..mono44.clone()
                },
            );
        }
        let mut after = egui::text::LayoutJob::default();
        for i in (active + 1).min(high)..high {
            after.append(&display_char(chars[i]), 0.0, mono44.clone());
        }
        let active_glyph = if has_active {
            match chars[active] {
                '\n' => " ".to_string(),
                '\t' => "⇥".to_string(),
                c => c.to_string(),
            }
        } else {
            String::new()
        };
        let mut active_job = egui::text::LayoutJob::default();
        if has_active {
            active_job.append(&active_glyph, 0.0, mono44.clone());
        }

        // Deterministic measurement (content coordinates, ±3px verified).
        let before_w = ui.ctx().fonts(|f| f.layout_job(before.clone()).size().x);
        let active_char_w = if has_active {
            ui.ctx().fonts(|f| f.layout_job(active_job).size().x)
        } else {
            0.0
        };
        let after_w = ui.ctx().fonts(|f| f.layout_job(after.clone()).size().x);
        // Active cell metrics (Swift CharacterCell: h7/v4 padding, radius 10).
        let line_h = ui
            .ctx()
            .fonts(|f| f.row_height(&egui::FontId::monospace(44.0)));
        let active_w = if has_active { active_char_w + 14.0 } else { 0.0 };
        let cell_h = line_h + 8.0;
        let content_w = before_w + active_w + after_w;
        // Finished lesson: keep focus on the last character.
        let anchor_cx = if has_active {
            before_w + active_w * 0.5
        } else {
            before_w
        };
        // Glide toward the pinned position (~0.12s ease-out, Swift
        // `withAnimation`); idle frames sit exactly on target.
        let target = pinned_shift(anchor_cx, width);
        if self.text_animating {
            let p = ((Instant::now() - self.text_anim_t0).as_secs_f32() / 0.12).min(1.0);
            if p >= 1.0 {
                self.text_shift = target;
                self.text_animating = false;
            } else {
                let eased = 1.0 - (1.0 - p).powi(3);
                self.text_shift =
                    self.text_anim_from + (target - self.text_anim_from) * eased;
                ui.ctx().request_repaint();
            }
        } else {
            self.text_shift = target;
        }
        let shift = self.text_shift;

        // Shifted single-line row, clipped to the region: the active
        // character is always exactly in the middle, nothing to scroll.
        let saved_clip = ui.clip_rect();
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(region.left() - shift, region.top()),
            egui::vec2(content_w.max(1.0), region.height()),
        );
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(region.intersect(saved_clip));
        row.spacing_mut().item_spacing.x = 0.0;
        row.label(before);
        if has_active {
            // Active cell painted directly (Swift CharacterCell background):
            // panel2 fill + brand outline + pending-gray glyph. A `Frame`
            // widget stretches to the full row height here, so it is never
            // used for this cell. On every switch the cell additionally pops
            // (1.18x -> 1.0 over ~0.14s) so each character change is visible
            // even at speed, when the glide alone would blur together.
            let pop = pop_scale(
                (Instant::now() - self.text_anim_t0).as_secs_f32(),
                self.text_animating,
            );
            let (slot, _) = row.allocate_exact_size(
                egui::vec2(active_w, cell_h),
                egui::Sense::hover(),
            );
            let cell = egui::Rect::from_center_size(slot.center(), slot.size() * pop);
            row.painter().rect_filled(cell, 10.0, theme::panel2(dark));
            row.painter().rect_stroke(
                cell,
                10.0,
                egui::Stroke::new(1.5_f32, theme::brand(dark)),
                egui::StrokeKind::Inside,
            );
            row.painter().text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                active_glyph,
                egui::FontId::monospace(44.0 * pop),
                theme::pending(dark),
            );
        }
        row.label(after);
        ui.set_clip_rect(saved_clip);
    }

    /// Open keyboard + finger legend in an exact 300pt block
    /// (Swift `keyboardBlock`). Geometry derives from the live available
    /// width every frame, so the board tracks window resizes.
    fn keyboard_block(&mut self, ui: &mut egui::Ui, dark: bool) {
        let avail_w = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(avail_w, 300.0),
            Layout::top_down(Align::Center),
            |ui| {
                self.keyboard_view(ui, dark);
                ui.add_space(12.0);
                // Finger legend, centered with explicit padding (a horizontal
                // row always spans the full width, so layout alignment alone
                // cannot center it — same as the keyboard rows above).
                let legend_items = [
                    (FingerZone::Pinky, self.t("finger_pinky")),
                    (FingerZone::Ring, self.t("finger_ring")),
                    (FingerZone::Middle, self.t("finger_middle")),
                    (FingerZone::Index, self.t("finger_index")),
                ];
                let mut legend_w = 0.0_f32;
                for (_, label) in &legend_items {
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        label,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(11.0),
                            color: theme::muted(dark),
                            ..Default::default()
                        },
                    );
                    legend_w += 10.0 + 5.0 + ui.ctx().fonts(|f| f.layout_job(job).size().x);
                }
                legend_w += 14.0 * (legend_items.len().saturating_sub(1) as f32);
                let legend_pad = ((avail_w - legend_w) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    ui.add_space(legend_pad);
                    for (zone, label) in &legend_items {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            let (r, _) = ui.allocate_exact_size(
                                egui::vec2(10.0, 10.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(
                                r.center(),
                                5.0,
                                theme::finger_tint(*zone, dark),
                            );
                            ui.label(
                                RichText::new(label)
                                    .size(11.0)
                                    .color(theme::muted(dark)),
                            );
                        });
                    }
                });
            },
        );
    }

    /// Full-size touch-typing keyboard: 15u hardware board capped at 920pt,
    /// square keycaps, flat finger tints, the next key in ink. Port of Swift
    /// `KeyboardView`. Every metric derives from the live available width,
    /// so keys, heights and fonts track window resizes exactly like Swift's
    /// `GeometryReader` metrics.
    fn keyboard_view(&self, ui: &mut egui::Ui, dark: bool) {
        let Some(layout) = self
            .layouts
            .get(&self.settings.keyboard_layout)
            .cloned()
            .or_else(|| self.layouts.values().next().cloned())
        else {
            ui.label(RichText::new("No keyboard layout").color(theme::muted(dark)));
            return;
        };
        let active: String = self
            .session
            .text
            .get(self.session.index)
            .map(|c| c.to_string())
            .unwrap_or_default();
        let rows = board_rows(&layout);
        if rows.is_empty() {
            return;
        }
        // Live metrics from the current width (Swift `GeometryReader`).
        // Rows are centered with explicit padding: a horizontal `ui.row`
        // always spans the full width, so layout alignment alone cannot
        // center narrower content.
        let avail_w = ui.available_width();
        let board_width = keyboard_board_width(avail_w);
        let m = keyboard_metrics(board_width);
        let pad = ((avail_w - board_width) / 2.0).max(0.0);
        ui.spacing_mut().item_spacing.y = m.row_gap;
        for row in &rows {
            let row_unit = m.row_unit(row.len());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = m.gap;
                ui.add_space(pad);
                for spec in row {
                    let w = (spec.units * row_unit).max(0.0);
                    let is_active = spec
                        .expects
                        .as_ref()
                        .map(|e| !active.is_empty() && e.to_lowercase() == active.to_lowercase())
                        .unwrap_or(false);
                    let fill = if is_active {
                        theme::ink(dark)
                    } else {
                        match spec.kind {
                            KeyKind::Key => theme::finger_tint(spec.zone, dark),
                            KeyKind::Space => theme::keycap(dark),
                            KeyKind::Modifier => theme::panel2(dark),
                        }
                    };
                    let fg = if is_active {
                        if dark { Color32::BLACK } else { Color32::WHITE }
                    } else {
                        match spec.kind {
                            KeyKind::Key => theme::ink(dark),
                            _ => theme::muted(dark),
                        }
                    };
                    let fs = if spec.kind == KeyKind::Modifier || spec.kind == KeyKind::Space {
                        m.mod_font
                    } else {
                        m.font
                    };
                    let mut text = RichText::new(spec.label.clone()).size(fs).color(fg);
                    if is_active || spec.kind == KeyKind::Key {
                        text = text.strong();
                    }
                    ui.add(
                        egui::Button::new(text)
                            .fill(fill)
                            .corner_radius(egui::CornerRadius::ZERO)
                            .min_size(egui::vec2(w, m.key_h)),
                    );
                }
            });
        }
        ui.add_space(12.0);
    }

    fn lesson_footer(&self, ui: &mut egui::Ui, dark: bool) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let progress = if self.session.text.is_empty() {
                0.0
            } else {
                self.session.index as f32 / self.session.text.len() as f32
            };
            ui.add(
                egui::ProgressBar::new(progress)
                    .fill(theme::brand(dark))
                    .desired_width(230.0),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!(
                    "{} / {}",
                    self.session.index,
                    self.session.text.len()
                ))
                .size(13.0)
                .color(theme::muted(dark)),
            );
        });
        ui.set_min_height(30.0);
    }
}

fn display_char(ch: char) -> String {
    match ch {
        '\n' => " ".to_string(),
        '\t' => "⇥".to_string(),
        c => c.to_string(),
    }
}

/// Row shift pinning the active character exactly in the middle of the
/// viewport (Swift `scrollTo(anchor: .center)`), without any scrolling.
fn pinned_shift(active_cx: f32, viewport_w: f32) -> f32 {
    active_cx - viewport_w * 0.5
}

/// Active-cell pop scale: 1.18x at the switch instant, easing to 1.0 over
/// ~0.14s. Pure function of elapsed wall time so it lands exactly even with
/// sparse frames; frozen at 1.0 once the switch animation releases.
fn pop_scale(elapsed_secs: f32, animating: bool) -> f32 {
    if !animating {
        return 1.0;
    }
    let q = (elapsed_secs / 0.14).min(1.0);
    1.0 + 0.18 * (1.0 - q).powi(2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Key,
    Space,
    Modifier,
}

#[derive(Debug, Clone)]
struct KeySpec {
    label: String,
    units: f32,
    zone: FingerZone,
    kind: KeyKind,
    expects: Option<String>,
}

/// Five hardware rows built from the layout's four letter rows plus a fixed
/// bottom modifier row — exact port of Swift `KeyboardView.boardRows`.
#[derive(Debug, Clone, Copy)]
struct KbMetrics {
    board: f32,
    gap: f32,
    row_gap: f32,
    key_h: f32,
    font: f32,
    mod_font: f32,
}

impl KbMetrics {
    /// Width of one key unit for a row with `keys` specs. Each row sums to
    /// 15u by construction; the unit divides the board minus this row's own
    /// gaps so rows align exactly (Swift `KeyboardView`).
    fn row_unit(&self, keys: usize) -> f32 {
        (self.board - self.gap * keys.saturating_sub(1) as f32) / 15.0
    }

    /// Total row width including gaps — always exactly the board width.
    /// Test helper for the fill invariant.
    #[cfg(test)]
    fn row_width(&self, units: f32, keys: usize) -> f32 {
        units * self.row_unit(keys) + self.gap * keys.saturating_sub(1) as f32
    }
}

/// Board width from the live available width (Swift `min(proxy.width, 920)`).
fn keyboard_board_width(available: f32) -> f32 {
    available.min(920.0)
}

/// Swift `KeyboardView.Metrics`: gap 6, row gap 7, heights/fonts derived
/// from the board width so everything tracks resizes.
fn keyboard_metrics(board_width: f32) -> KbMetrics {
    let gap = 6.0_f32;
    let row_gap = 7.0_f32;
    // Nominal unit off a full 15-key row; heights stay uniform.
    let unit = ((board_width - gap * 14.0) / 15.0).max(20.0);
    let key_h = (unit * 0.66).clamp(30.0, 42.0);
    KbMetrics {
        board: board_width,
        gap,
        row_gap,
        key_h,
        font: (key_h * 0.42).clamp(10.0, 16.0),
        mod_font: (key_h * 0.30).clamp(9.0, 12.0),
    }
}

fn board_rows(layout: &KeyboardLayout) -> Vec<Vec<KeySpec>> {
    const TOTAL: f32 = 15.0;
    if layout.rows.len() < 4 {
        return Vec::new();
    }
    let numbers = &layout.rows[0];
    let upper = &layout.rows[1];
    let home = &layout.rows[2];
    let lower = &layout.rows[3];

    let letter_keys = |keys: &[String], row: usize| -> Vec<KeySpec> {
        keys.iter()
            .enumerate()
            .map(|(col, key)| KeySpec {
                label: key.to_uppercase(),
                units: 1.0,
                zone: theme::finger_zone(row, col, keys.len()),
                kind: KeyKind::Key,
                expects: Some(key.clone()),
            })
            .collect::<Vec<_>>()
    };

    let mut rows: Vec<Vec<KeySpec>> = Vec::new();
    // Number row + 2u Backspace.
    rows.push(
        letter_keys(numbers, 0)
            .into_iter()
            .chain(std::iter::once(KeySpec {
                label: "Backspace".to_string(),
                units: TOTAL - numbers.len() as f32,
                zone: FingerZone::Pinky,
                kind: KeyKind::Modifier,
                expects: None,
            }))
            .collect(),
    );
    // Tab row: 1.5u Tab, then letters; ANSI `|\` widens to 1.5u.
    {
        let mut specs = vec![KeySpec {
            label: "Tab".to_string(),
            units: 1.5,
            zone: FingerZone::Pinky,
            kind: KeyKind::Modifier,
            expects: Some("\t".to_string()),
        }];
        let mut letters = letter_keys(upper, 1);
        let mut trailing = TOTAL - 1.5 - upper.len() as f32;
        if trailing < 1.25 && !letters.is_empty() {
            if let Some(last) = letters.last_mut() {
                last.units = 1.5;
            }
            trailing = TOTAL - 1.5 - (letters.len() as f32 - 1.0) - 1.5;
        }
        specs.extend(letters);
        if trailing > 0.01 {
            specs.push(KeySpec {
                label: String::new(),
                units: trailing,
                zone: FingerZone::Pinky,
                kind: KeyKind::Modifier,
                expects: None,
            });
        }
        rows.push(specs);
    }
    // Home row: 1.75u Caps, letters, Enter fills the rest.
    rows.push(
        std::iter::once(KeySpec {
            label: "Caps".to_string(),
            units: 1.75,
            zone: FingerZone::Pinky,
            kind: KeyKind::Modifier,
            expects: None,
        })
        .chain(letter_keys(home, 2))
        .chain(std::iter::once(KeySpec {
            label: "Enter".to_string(),
            units: TOTAL - 1.75 - home.len() as f32,
            zone: FingerZone::Pinky,
            kind: KeyKind::Modifier,
            expects: Some("\n".to_string()),
        }))
        .collect(),
    );
    // Bottom letter row: 2.25u Shift, letters, wide right Shift.
    rows.push(
        std::iter::once(KeySpec {
            label: "Shift".to_string(),
            units: 2.25,
            zone: FingerZone::Pinky,
            kind: KeyKind::Modifier,
            expects: None,
        })
        .chain(letter_keys(lower, 3))
        .chain(std::iter::once(KeySpec {
            label: "Shift".to_string(),
            units: TOTAL - 2.25 - lower.len() as f32,
            zone: FingerZone::Pinky,
            kind: KeyKind::Modifier,
            expects: None,
        }))
        .collect(),
    );
    // Modifier row: Ctrl Win Alt | space | Alt Win Menu Ctrl.
    {
        let edge = 1.25_f32;
        let space_label = if layout.space_label.is_empty() {
            "SPACE".to_string()
        } else {
            layout.space_label.clone()
        };
        rows.push(vec![
            KeySpec { label: "Ctrl".into(), units: edge, zone: FingerZone::Pinky, kind: KeyKind::Modifier, expects: None },
            KeySpec { label: "Win".into(), units: edge, zone: FingerZone::Pinky, kind: KeyKind::Modifier, expects: None },
            KeySpec { label: "Alt".into(), units: edge, zone: FingerZone::Pinky, kind: KeyKind::Modifier, expects: None },
            KeySpec { label: space_label, units: TOTAL - edge * 7.0, zone: FingerZone::Thumb, kind: KeyKind::Space, expects: Some(" ".to_string()) },
            KeySpec { label: "Alt".into(), units: edge, zone: FingerZone::Pinky, kind: KeyKind::Modifier, expects: None },
            KeySpec { label: "Win".into(), units: edge, zone: FingerZone::Pinky, kind: KeyKind::Modifier, expects: None },
            KeySpec { label: "Menu".into(), units: edge, zone: FingerZone::Pinky, kind: KeyKind::Modifier, expects: None },
            KeySpec { label: "Ctrl".into(), units: edge, zone: FingerZone::Pinky, kind: KeyKind::Modifier, expects: None },
        ]);
    }
    rows
}

// ---------------------------------------------------------------------------
// Statistics page (Swift `StatsView`: matte title + summary + table)
// ---------------------------------------------------------------------------

/// Stats table column starts, as fractions of the row width (measured off
/// the Swift table header positions).
const STATS_COLS: [f32; 6] = [0.02, 0.15, 0.355, 0.545, 0.735, 0.845];
/// One stats table row: six cells at the Swift fractional column starts,
/// vertically centered, clipped to the row (explicit offsets — a horizontal
/// row always spans the full width, so alignment alone cannot place cells).
fn stats_row_cells(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    cells: [&str; 6],
    size: f32,
    color: Color32,
) {
    let saved_clip = ui.clip_rect();
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.set_clip_rect(rect.intersect(saved_clip));
    row.spacing_mut().item_spacing.x = 0.0;
    let mut cursor = 0.0_f32;
    for (c, text) in cells.iter().enumerate() {
        let x = STATS_COLS[c] * rect.width();
        if x > cursor {
            row.add_space(x - cursor);
            cursor = x;
        }
        let mut job = egui::text::LayoutJob::default();
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(size),
                ..Default::default()
            },
        );
        let w = row.ctx().fonts(|f| f.layout_job(job).size().x);
        row.label(RichText::new(*text).size(size).color(color));
        cursor += w;
    }
    ui.set_clip_rect(saved_clip);
}
/// Pill row height; gaps distribute so the table always fills its box.
const STATS_ROW_H: f32 = 32.0;

impl BitTypingApp {
    fn stats_page(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark(ui.ctx());
        // Explicit column like the lesson page: `available_height` inside
        // auto-sized nesting reads back ~0, so measure once at panel level.
        let top_w = ui.available_width();
        let top_h = ui.available_height();
        let column_w = (top_w - 24.0).max(100.0);
        let left_pad = ((top_w - column_w) / 2.0).max(0.0);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(left_pad);
            ui.allocate_ui_with_layout(
                egui::vec2(column_w, (top_h - 4.0).max(100.0)),
                Layout::top_down(Align::Min),
                |ui| {
                    // Plain page title — in content, not the toolbar.
                    ui.label(
                        RichText::new(self.t("your_progress"))
                            .size(22.0)
                            .strong()
                            .color(theme::ink(dark)),
                    );
                    ui.label(
                        RichText::new(self.history_summary())
                            .size(13.0)
                            .color(theme::muted(dark)),
                    );
                    ui.add_space(8.0);
                    // Fixed header row (stays put while the rows scroll).
                    let row_w = ui.available_width();
                    let headers = [
                        self.t("date"),
                        self.t("lesson"),
                        self.t(self.speed_unit_key()),
                        self.t("accuracy"),
                        self.t("done"),
                        self.t("errors"),
                    ];
                    let header_refs = [
                        headers[0].as_str(),
                        headers[1].as_str(),
                        headers[2].as_str(),
                        headers[3].as_str(),
                        headers[4].as_str(),
                        headers[5].as_str(),
                    ];
                    let (header_rect, _) = ui.allocate_exact_size(
                        egui::vec2(row_w, 22.0),
                        egui::Sense::hover(),
                    );
                    stats_row_cells(ui, header_rect, header_refs, 12.0, theme::muted(dark));
                    ui.separator();
                    ui.add_space(4.0);
                    // Rows fill the whole remaining box (Swift table): data rows
                    // alternate filled/uncolored, the rest are empty filler pills
                    // exactly like the Swift empty state.
                    let remaining = ui.available_height();
                    let slots = ((remaining / 46.0).floor() as usize).max(1);
                    let data = self.history.len();
                    let total = slots.max(data);
                    let gap = if data <= slots && slots > 1 {
                        ((remaining - slots as f32 * STATS_ROW_H) / (slots as f32 - 1.0))
                            .clamp(4.0, 60.0)
                    } else {
                        14.0
                    };
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for i in 0..total {
                                let (row_rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(row_w, STATS_ROW_H),
                                    egui::Sense::click(),
                                );
                                let data_idx = data.checked_sub(1 + i);
                                let is_filler = data_idx.is_none();
                                // black;uncolored alternating data rows, all
                                // filler rows filled (Swift empty state).
                                if is_filler || i % 2 == 0 {
                                    ui.painter().rect_filled(
                                        row_rect,
                                        8.0,
                                        theme::panel2(dark),
                                    );
                                }
                                if let Some(idx) = data_idx {
                                    if self.stats_selected == Some(idx) {
                                        ui.painter().rect_filled(
                                            row_rect,
                                            8.0,
                                            egui::Color32::from_rgba_unmultiplied(
                                                10, 132, 255, 70,
                                            ),
                                        );
                                    }
                                    let attempt = &self.history[idx];
                                    let speed = self.format_speed(attempt.wpm);
                                    let acc = format!("{:.1}%", attempt.accuracy);
                                    let done = format!("{:.0}%", attempt.done);
                                    let errs = format!("{}", attempt.errors);
                                    let date = attempt.finished_at.replace('T', " ");
                                    let cells = [
                                        date.as_str(),
                                        attempt.lesson.as_str(),
                                        speed.as_str(),
                                        acc.as_str(),
                                        done.as_str(),
                                        errs.as_str(),
                                    ];
                                    stats_row_cells(ui, row_rect, cells, 13.0, theme::ink(dark));
                                    if resp.double_clicked() {
                                        self.results_tab = ResultsTab::Overview;
                                        self.chart_metric = ChartMetric::Speed;
                                        self.results = Some(attempt.clone());
                                    } else if resp.clicked() {
                                        self.stats_selected = Some(idx);
                                    }
                                }
                                if i + 1 < total {
                                    ui.add_space(gap);
                                }
                            }
                        });
                },
            );
            ui.add_space(12.0);
        });
    }

    fn clear_history_dialog(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        egui::Window::new(self.t("clear_history_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(self.t("clear_history_body"));
                ui.horizontal(|ui| {
                    if ui
                        .add(theme::secondary_button(
                            RichText::new(self.t("cancel")).size(13.0),
                            dark,
                        ))
                        .clicked()
                    {
                        self.confirm_clear_history = false;
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(self.t("clear_history"))
                                    .size(13.0)
                                    .color(Color32::WHITE),
                            )
                            .fill(theme::error(dark))
                            .corner_radius(egui::CornerRadius::same(16))
                            .min_size(egui::vec2(0.0, 32.0)),
                        )
                        .clicked()
                    {
                        self.history.clear();
                        let _ = save_history(&paths::history_file(&self.root), &self.history);
                        self.stats_selected = None;
                        self.confirm_clear_history = false;
                    }
                });
            });
    }
}

// ---------------------------------------------------------------------------
// Editor page (Swift `EditorView`: title + name field + glass text panel)
// ---------------------------------------------------------------------------

impl BitTypingApp {
    fn load_editor_from_current(&mut self) {
        if let Some(path) = self.current_path() {
            self.editor_name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.editor_text = read_course_text(&path).unwrap_or_default();
        }
    }

    fn editor_page(&mut self, ui: &mut egui::Ui) {
        let dark = self.dark(ui.ctx());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.vertical(|ui| {
                // Plain page title — in content, not the toolbar.
                ui.label(
                    RichText::new(self.t("lesson_editor"))
                        .size(22.0)
                        .strong()
                        .color(theme::ink(dark)),
                );
                ui.add_space(6.0);
                let name_hint = self.t("lesson_name");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.editor_name)
                            .hint_text(name_hint)
                            .desired_width(320.0),
                    );
                });
                ui.add_space(8.0);
                if !self.save_error.is_empty() {
                    ui.label(RichText::new(&self.save_error).color(theme::error(dark)));
                }
                // The text surface is the one panel here: rounded, no hairline.
                egui::Frame::new()
                    .fill(theme::panel(dark))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        egui::ScrollArea::both().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.editor_text)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(22)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });
                    });
            });
            ui.add_space(12.0);
        });
    }

    fn save_editor_lesson(&mut self) {
        let safe = sanitise_lesson_name(&self.editor_name);
        if safe.is_empty() || self.editor_text.is_empty() {
            self.save_error = format!(
                "{}: {}",
                self.t("cannot_save_title"),
                self.t("cannot_save_body")
            );
            return;
        }
        let path = paths::courses_dir(&self.root).join(format!("{safe}.txt"));
        let current = self.current_path().map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string()).unwrap_or_default();
        if path.exists() && path.file_stem().map(|s| s.to_string_lossy().to_string()) != Some(current.clone()) {
            self.confirm_replace = Some(path);
            return;
        }
        self.write_editor_file(&path);
    }

    fn write_editor_file(&mut self, path: &Path) {
        if std::fs::write(path, &self.editor_text).is_err() {
            self.save_error = self.t("cannot_save_title");
            return;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if !BUILTIN_COURSE_FILES.contains(&name) {
                self.custom.insert(name.to_string());
                let _ = courses::save_custom_courses(
                    &paths::custom_courses_file(&self.root),
                    &self.custom,
                );
            }
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            self.reload_courses(Some(&stem));
        }
    }

    fn replace_dialog(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        let Some(path) = self.confirm_replace.clone() else {
            return;
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        egui::Window::new(self.t("replace_lesson_title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    self.t("replace_lesson_body").replace("{name}", &name),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add(theme::secondary_button(
                            RichText::new(self.t("cancel")).size(13.0),
                            dark,
                        ))
                        .clicked()
                    {
                        self.confirm_replace = None;
                    }
                    if ui
                        .add(theme::primary_button(
                            RichText::new(self.t("save_lesson")).size(13.0),
                            dark,
                        ))
                        .clicked()
                    {
                        self.confirm_replace = None;
                        self.write_editor_file(&path);
                    }
                });
            });
    }

    fn import_pending(&mut self, to_editor: bool) {
        let pending = std::mem::take(&mut self.pending_imports);
        let mut last: Option<String> = None;
        for src in pending {
            let name = src.file_name().unwrap_or_default().to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            match read_course_text(&src) {
                Ok(text) => {
                    let dest = paths::courses_dir(&self.root).join(&name);
                    if std::fs::write(&dest, text).is_ok() {
                        self.custom.insert(name.clone());
                        last = dest
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string());
                    }
                }
                Err(e) => {
                    self.save_error = self
                        .t("import_failed_body")
                        .replace("{name}", &name)
                        .replace("{error}", &e);
                }
            }
        }
        let _ = courses::save_custom_courses(
            &paths::custom_courses_file(&self.root),
            &self.custom,
        );
        // Import is silent on overwrite; the lesson list simply refreshes.
        self.reload_courses(last.as_deref());
        if to_editor {
            self.page = Page::Editor;
            self.load_editor_from_current();
        }
    }
}

// ---------------------------------------------------------------------------
// Options / setup / about (Swift sheets)
// ---------------------------------------------------------------------------

impl BitTypingApp {
    fn language_codes(&self) -> Vec<String> {
        self.i18n.codes().to_vec()
    }

    fn keyboard_codes(&self) -> Vec<String> {
        self.layouts.keys().cloned().collect()
    }

    fn localized_typo(&self, mode: &str) -> String {
        match mode {
            "Type the right character" => self.t("typo_right"),
            "Correct with Backspace" => self.t("typo_backspace"),
            "Continue" => self.t("typo_continue"),
            _ => mode.to_string(),
        }
    }

    fn options_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        let mut open = self.show_options;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Header: title left, Cancel secondary right (Swift OptionsView).
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.t("options"))
                            .size(22.0)
                            .strong()
                            .color(theme::ink(dark)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(theme::secondary_button(
                                RichText::new(self.t("cancel")).size(13.0),
                                dark,
                            ))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .show(ui, |ui| {
                        self.options_form_full(ui, dark);
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(theme::primary_button(
                                RichText::new(format!("✔ {}", self.t("save_options"))).size(13.0),
                                dark,
                            ))
                            .clicked()
                        {
                            save = true;
                        }
                        if ui
                            .add(theme::secondary_button(
                                RichText::new(format!("× {}", self.t("cancel"))).size(13.0),
                                dark,
                            ))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });
        self.show_options = open && !save && !cancel;
        if cancel {
            self.show_options = false;
            self.sync_options_form();
        }
        if save && self.apply_options_form() {
            self.show_options = false;
        }
    }

    /// Full options form (Swift `OptionsView` Form): behavior section with
    /// pickers, numeric fields and toggles.
    fn options_form_full(&mut self, ui: &mut egui::Ui, dark: bool) {
        let lang_label = self.t("language");
        let kb_label = self.t("keyboard_layout");
        let typo_label = self.t("typo_behavior");
        let goal_speed = self.t("goal_speed");
        let goal_accuracy = self.t("goal_accuracy");
        let slow_threshold = self.t("slow_threshold");
        let timed_lesson = self.t("timed_lesson");
        let auto_start = self.t("auto_start");
        let allow_backspace = self.t("allow_backspace");
        let sound_feedback = self.t("sound_feedback");
        let metronome = self.t("metronome");
        let behavior = self.t("lesson_behavior");
        let lang_codes = self.language_codes();
        let lang_names: Vec<(String, String)> = lang_codes
            .iter()
            .map(|c| (c.clone(), self.i18n.display_name(c)))
            .collect();
        let kb_codes = self.keyboard_codes();
        let typo_modes: Vec<(String, String)> = TYPO_MODES
            .iter()
            .map(|m| (m.to_string(), self.localized_typo(m)))
            .collect();
        let current_typo_label = self.localized_typo(&self.opt_typo.clone());
        let current_lang_name = self.i18n.display_name(&self.opt_language.clone());
        let prev_lang = self.opt_language.clone();

        ui.label(
            RichText::new(behavior.to_uppercase())
                .size(12.0)
                .strong()
                .color(theme::muted(dark)),
        );
        ui.add_space(2.0);
        theme::card_frame(dark).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(lang_label).color(theme::ink(dark)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("opt_lang")
                        .selected_text(current_lang_name)
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for (code, name) in &lang_names {
                                ui.selectable_value(&mut self.opt_language, code.clone(), name);
                            }
                        });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new(kb_label).color(theme::ink(dark)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let selected = self.opt_keyboard.clone();
                    egui::ComboBox::from_id_salt("opt_kb")
                        .selected_text(selected)
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for code in &kb_codes {
                                ui.selectable_value(&mut self.opt_keyboard, code.clone(), code);
                            }
                        });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new(typo_label).color(theme::ink(dark)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("opt_typo")
                        .selected_text(current_typo_label)
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for (mode, label) in &typo_modes {
                                ui.selectable_value(&mut self.opt_typo, mode.clone(), label);
                            }
                        });
                });
            });
            ui.separator();
            for (label, value) in [
                (goal_speed, &mut self.opt_goal_wpm as *mut String),
                (goal_accuracy, &mut self.opt_goal_accuracy as *mut String),
                (slow_threshold, &mut self.opt_timeout as *mut String),
                (timed_lesson, &mut self.opt_timed as *mut String),
            ] {
                // SAFETY-free: disjoint fields, single frame, no reentrancy.
                let field = unsafe { &mut *value };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&label).color(theme::ink(dark)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(egui::TextEdit::singleline(field).desired_width(140.0));
                    });
                });
                ui.separator();
            }
            for (label, value) in [
                (auto_start, &mut self.opt_auto_start as *mut bool),
                (allow_backspace, &mut self.opt_backspace as *mut bool),
                (sound_feedback, &mut self.opt_sound as *mut bool),
                (metronome, &mut self.opt_metronome as *mut bool),
            ] {
                let flag = unsafe { &mut *value };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&label).color(theme::ink(dark)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        theme::toggle_switch(ui, flag, dark);
                    });
                });
                ui.separator();
            }
        });
        // Auto-switch default keyboard when the language changes (Swift parity).
        if self.opt_language != prev_lang {
            if let Some(kb) = self.i18n.default_keyboard(&self.opt_language.clone()) {
                if self.layouts.contains_key(&kb) {
                    self.opt_keyboard = kb;
                }
            }
        }
        if !self.opt_error.is_empty() {
            let err = self.opt_error.clone();
            ui.label(RichText::new(err).color(theme::error(dark)));
        }
    }

    fn setup_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        let mut finish = false;
        egui::Window::new("")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .default_width(640.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Header: bundled app icon (Swift SetupView brand mark).
                ui.horizontal(|ui| {
                    if let Some(icon) = &self.brand_icon {
                        ui.add(
                            egui::Image::new(icon)
                                .max_size(egui::vec2(56.0, 56.0))
                                .corner_radius(egui::CornerRadius::same(14)),
                        );
                    } else {
                        egui::Frame::new()
                            .fill(theme::ink(dark))
                            .corner_radius(egui::CornerRadius::same(14))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("⌨")
                                        .size(32.0)
                                        .color(if dark { Color32::BLACK } else { Color32::WHITE }),
                                );
                            });
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(self.t("setup_welcome"))
                                .size(24.0)
                                .strong()
                                .color(theme::ink(dark)),
                        );
                        ui.label(
                            RichText::new(self.t("setup_intro"))
                                .size(14.0)
                                .color(theme::muted(dark)),
                        );
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        self.setup_form(ui, dark);
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.t("setup_change_later"))
                            .size(13.0)
                            .color(theme::muted(dark)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(theme::primary_button(
                                RichText::new(format!("✔ {}", self.t("finish_setup"))).size(13.0),
                                dark,
                            ))
                            .clicked()
                        {
                            finish = true;
                        }
                    });
                });
            });
        if finish && self.apply_options_form() {
            self.settings.setup_complete = true;
            self.save_settings();
            self.show_setup = false;
        }
    }

    /// Setup form: language section + preferences section (Swift `SetupView`).
    /// No slow-threshold / timed-lesson rows — matching Swift.
    fn setup_form(&mut self, ui: &mut egui::Ui, dark: bool) {
        let lang_heading = self.t("setup_language_heading");
        let pref_heading = self.t("setup_preferences_heading");
        let lang_label = self.t("language");
        let kb_label = self.t("keyboard_layout");
        let typo_label = self.t("typo_behavior");
        let goal_speed = self.t("goal_speed");
        let goal_accuracy = self.t("goal_accuracy");
        let auto_start = self.t("auto_start");
        let allow_backspace = self.t("allow_backspace");
        let sound_feedback = self.t("sound_feedback");
        let metronome = self.t("metronome");
        let lang_codes = self.language_codes();
        let lang_names: Vec<(String, String)> = lang_codes
            .iter()
            .map(|c| {
                let name = self.i18n.display_name(c);
                (c.clone(), if name == *c { c.clone() } else { format!("{name} ({c})") })
            })
            .collect();
        let kb_codes = self.keyboard_codes();
        let kb_names: Vec<(String, String)> = kb_codes
            .iter()
            .map(|c| {
                let name = self
                    .layouts
                    .get(c)
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                (c.clone(), if name.is_empty() { c.clone() } else { format!("{name} ({c})") })
            })
            .collect();
        let typo_modes: Vec<(String, String)> = TYPO_MODES
            .iter()
            .map(|m| (m.to_string(), self.localized_typo(m)))
            .collect();
        let current_typo_label = self.localized_typo(&self.opt_typo.clone());
        let current_lang = self
            .i18n
            .display_name(&self.opt_language.clone());
        let current_lang_full = lang_names
            .iter()
            .find(|(c, _)| *c == self.opt_language)
            .map(|(_, n)| n.clone())
            .unwrap_or(current_lang);
        let current_kb_full = kb_names
            .iter()
            .find(|(c, _)| *c == self.opt_keyboard)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| self.opt_keyboard.clone());
        let prev_lang = self.opt_language.clone();

        // Section 1: language + keyboard.
        ui.label(
            RichText::new(lang_heading.to_uppercase())
                .size(12.0)
                .strong()
                .color(theme::muted(dark)),
        );
        ui.add_space(2.0);
        theme::card_frame(dark).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(lang_label.clone()).color(theme::ink(dark)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("setup_lang")
                        .selected_text(current_lang_full.clone())
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for (code, name) in &lang_names {
                                ui.selectable_value(&mut self.opt_language, code.clone(), name);
                            }
                        });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new(kb_label.clone()).color(theme::ink(dark)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("setup_kb")
                        .selected_text(current_kb_full.clone())
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for (code, name) in &kb_names {
                                ui.selectable_value(&mut self.opt_keyboard, code.clone(), name);
                            }
                        });
                });
            });
            ui.separator();
        });
        ui.add_space(8.0);
        // Section 2: practice preferences.
        ui.label(
            RichText::new(pref_heading.to_uppercase())
                .size(12.0)
                .strong()
                .color(theme::muted(dark)),
        );
        ui.add_space(2.0);
        theme::card_frame(dark).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(typo_label.clone()).color(theme::ink(dark)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("setup_typo")
                        .selected_text(current_typo_label.clone())
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for (mode, label) in &typo_modes {
                                ui.selectable_value(&mut self.opt_typo, mode.clone(), label);
                            }
                        });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new(goal_speed.clone()).color(theme::ink(dark)));
                ui.add(egui::TextEdit::singleline(&mut self.opt_goal_wpm).desired_width(80.0));
                ui.add_space(12.0);
                ui.label(RichText::new(goal_accuracy.clone()).color(theme::ink(dark)));
                ui.add(
                    egui::TextEdit::singleline(&mut self.opt_goal_accuracy)
                        .desired_width(80.0),
                );
            });
            ui.separator();
            for (label, value) in [
                (auto_start.clone(), &mut self.opt_auto_start as *mut bool),
                (allow_backspace.clone(), &mut self.opt_backspace as *mut bool),
                (sound_feedback.clone(), &mut self.opt_sound as *mut bool),
                (metronome.clone(), &mut self.opt_metronome as *mut bool),
            ] {
                let flag = unsafe { &mut *value };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&label).color(theme::ink(dark)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        theme::toggle_switch(ui, flag, dark);
                    });
                });
                ui.separator();
            }
        });
        ui.add_space(8.0);
        if self.opt_language != prev_lang {
            if let Some(kb) = self.i18n.default_keyboard(&self.opt_language.clone()) {
                if self.layouts.contains_key(&kb) {
                    self.opt_keyboard = kb;
                }
            }
        }
        if !self.opt_error.is_empty() {
            let err = self.opt_error.clone();
            ui.label(RichText::new(err).color(theme::error(dark)));
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        let mut open = self.show_about;
        let mut close = false;
        let title = self.t("about_bit_typing");
        let close_label = self.t("close");
        let version_line = format!("{} {}", self.t("version"), crate::APP_VERSION);
        let description = self.t("about_description");
        let website_label = format!("○ {}: bitiskola.github.io/bit-typing", self.t("website"));
        let devs_title = self.t("developers_contributors");
        let role_maintainer = self.t("maintainer_developer");
        let role_icon = self.t("icon_designer");
        // Fixed 620x600 sheet like Swift `AboutView`; the lower part scrolls.
        egui::Window::new("")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .fixed_size([620.0, 600.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(title)
                            .size(22.0)
                            .strong()
                            .color(theme::ink(dark)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(theme::secondary_button(
                                RichText::new(close_label).size(13.0),
                                dark,
                            ))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            // Brand icon: bundled favico.png, rounded 22.
                            if let Some(icon) = &self.brand_icon {
                                ui.add(
                                    egui::Image::new(icon)
                                        .max_size(egui::vec2(96.0, 96.0))
                                        .corner_radius(egui::CornerRadius::same(22)),
                                );
                            } else {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(96.0, 96.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    22.0,
                                    theme::panel2(dark),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "⌨".to_string(),
                                    egui::FontId::new(
                                        56.0,
                                        egui::FontFamily::Proportional,
                                    ),
                                    theme::ink(dark),
                                );
                            }
                            ui.label(
                                RichText::new("BIT Typing")
                                    .size(26.0)
                                    .strong()
                                    .color(theme::ink(dark)),
                            );
                            ui.label(
                                RichText::new(version_line)
                                    .size(14.0)
                                    .strong()
                                    .color(theme::muted(dark)),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(description)
                                    .size(14.0)
                                    .color(theme::muted(dark)),
                            );
                            ui.add_space(6.0);
                            if ui
                                .add(theme::secondary_button(
                                    RichText::new(website_label).size(13.0),
                                    dark,
                                ))
                                .clicked()
                            {
                                let _ = open::that(crate::APP_WEBSITE);
                            }
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(devs_title)
                                    .size(18.0)
                                    .strong()
                                    .color(theme::ink(dark)),
                            );
                            ui.add_space(4.0);
                        });
                        // Two cards side by side; columns split the bounded
                        // width equally so the sheet can never balloon.
                        ui.columns(2, |cols| {
                            team_card(
                                &mut cols[0],
                                dark,
                                "micr0softstore",
                                &role_maintainer,
                                "MS",
                                self.avatar_ms.as_ref(),
                            );
                            team_card(
                                &mut cols[1],
                                dark,
                                "cacto.tsx",
                                &role_icon,
                                "C",
                                self.avatar_cacto.as_ref(),
                            );
                        });
                        ui.add_space(8.0);
                    });
            });
        if close {
            open = false;
        }
        self.show_about = open;
    }

    fn well_done_overlay(&self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        egui::Window::new("")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .fixed_size([480.0, 330.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    // Ink circle + checkmark (Swift WellDoneView). Painter
                    // circle: a Frame would stretch to full width here.
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(120.0, 120.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().circle_filled(
                        rect.center(),
                        60.0,
                        theme::ink(dark),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "✔".to_string(),
                        egui::FontId::new(52.0, egui::FontFamily::Proportional),
                        if dark { Color32::BLACK } else { Color32::WHITE },
                    );
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(self.t("well_done"))
                            .size(28.0)
                            .strong()
                            .color(theme::ink(dark)),
                    );
                    ui.label(
                        RichText::new(self.t("results_loading"))
                            .size(13.0)
                            .color(theme::muted(dark)),
                    );
                    ui.add_space(30.0);
                });
            });
    }
}

fn team_card(
    ui: &mut egui::Ui,
    dark: bool,
    name: &str,
    role: &str,
    initials: &str,
    avatar: Option<&egui::TextureHandle>,
) {
    // Fill the column width so the card never measures unbounded.
    let content_w = (ui.available_width() - 24.0).max(80.0);
    theme::card_frame(dark).show(ui, |ui| {
        ui.set_width(content_w);
        ui.vertical_centered(|ui| {
            ui.add_space(14.0);
            avatar_circle(ui, dark, 84.0, initials, avatar);
            ui.add_space(4.0);
            ui.label(RichText::new(name).size(16.0).strong().color(theme::ink(dark)));
            ui.label(RichText::new(role).size(13.0).color(theme::muted(dark)));
            ui.add_space(14.0);
        });
    });
}

/// 84pt profile picture: bundled photo cropped to a circle, or a panel2
/// circle with initials when the asset is missing/undecodable.
/// Exact-sized allocations — never a stretching `Frame` (see About sheet).
fn avatar_circle(
    ui: &mut egui::Ui,
    dark: bool,
    diameter: f32,
    initials: &str,
    avatar: Option<&egui::TextureHandle>,
) {
    if let Some(tex) = avatar {
        ui.add(
            egui::Image::new(tex)
                .max_size(egui::vec2(diameter, diameter))
                .corner_radius(egui::CornerRadius::same((diameter / 2.0) as u8)),
        );
    } else {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), diameter / 2.0, theme::panel2(dark));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initials.to_string(),
            egui::FontId::new(28.0, egui::FontFamily::Proportional),
            theme::ink(dark),
        );
    }
}

// ---------------------------------------------------------------------------
// Results (Swift `ResultsView`: score header, gauges, tabs, footer)
// ---------------------------------------------------------------------------

impl BitTypingApp {
    fn results_window(&mut self, ctx: &egui::Context) {
        let dark = self.dark(ctx);
        let Some(attempt) = self.results.clone() else {
            return;
        };
        let score = overall_score(
            attempt.wpm,
            attempt.accuracy,
            attempt.slowdown,
            self.settings.goal_wpm,
        );
        let next = next_lesson(
            &self.course_paths,
            &self.custom,
            &attempt.course_file,
            &attempt.lesson,
            attempt.custom_course,
        );
        let mut open = true;
        let mut go_home = false;
        let mut go_next: Option<PathBuf> = None;
        egui::Window::new("")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .title_bar(false)
            .default_size([760.0, 620.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Header: title left, score + stars right.
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.t("lesson_results"))
                            .size(24.0)
                            .strong()
                            .color(theme::ink(dark)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        stars_view(ui, score, dark);
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("{score:.0}%"))
                                .size(20.0)
                                .strong()
                                .color(theme::ink(dark)),
                        );
                    });
                });
                ui.add_space(6.0);
                // Gauges card: overall in brand blue, the rest monochrome ink.
                theme::card_frame(dark).show(ui, |ui| {
                    ui.vertical(|ui| {
                        gauge_view(
                            ui,
                            &self.t("overall_score"),
                            &format!("{score:.0}%"),
                            score / 100.0,
                            theme::brand(dark),
                            dark,
                        );
                        gauge_view(
                            ui,
                            &capitalized(&self.t("speed")),
                            &format!(
                                "{} {}",
                                self.format_speed(attempt.wpm),
                                self.t(self.speed_unit_key())
                            ),
                            (attempt.wpm / self.settings.goal_wpm.max(1.0)).min(1.0),
                            theme::ink(dark),
                            dark,
                        );
                        gauge_view(
                            ui,
                            &capitalized(&self.t("accuracy")),
                            &format!("{:.1}%", attempt.accuracy),
                            attempt.accuracy / 100.0,
                            theme::ink(dark),
                            dark,
                        );
                        gauge_view(
                            ui,
                            &self.t("slowdown"),
                            &format!("{:.1}%", attempt.slowdown),
                            1.0 - attempt.slowdown / 100.0,
                            theme::ink(dark),
                            dark,
                        );
                    });
                });
                ui.add_space(10.0);
                // Tabs: primary selected / secondary rest, with icons,
                // centered like Swift (a full-width row left-packs, so pad
                // explicitly from measured label widths).
                let tabs = [
                    (ResultsTab::Overview, "☰", self.t("overview")),
                    (ResultsTab::Details, "📊", self.t("detailed_statistics")),
                    (ResultsTab::Errors, "⚠", self.t("errors_overview")),
                ];
                let mut tabs_w = 0.0_f32;
                for (_, icon, label) in &tabs {
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &format!("{icon} {label}"),
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(13.0),
                            color: theme::ink(dark),
                            ..Default::default()
                        },
                    );
                    // Pill width ~= label + button side padding (14 each).
                    tabs_w += ui.ctx().fonts(|f| f.layout_job(job).size().x) + 28.0;
                }
                tabs_w += 8.0 * (tabs.len().saturating_sub(1) as f32);
                let tabs_pad = ((ui.available_width() - tabs_w) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.add_space(tabs_pad);
                    for (tab, icon, label) in tabs {
                        let selected = self.results_tab == tab;
                        let btn = if selected {
                            theme::primary_button(
                                RichText::new(format!("{icon} {label}")).size(13.0),
                                dark,
                            )
                        } else {
                            theme::secondary_button(
                                RichText::new(format!("{icon} {label}")).size(13.0),
                                dark,
                            )
                        };
                        if ui.add(btn).clicked() {
                            self.results_tab = tab;
                        }
                    }
                });
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| match self.results_tab {
                        ResultsTab::Overview => self.results_overview(ui, &attempt, dark),
                        ResultsTab::Details => self.results_details(ui, &attempt, dark),
                        ResultsTab::Errors => self.results_errors(ui, &attempt, dark),
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(next_path) = next.clone() {
                            if ui
                                .add(theme::brand_button(
                                    RichText::new(format!("➡ {}", self.t("next_lesson"))).size(13.0),
                                    dark,
                                ))
                                .clicked()
                            {
                                go_next = Some(next_path);
                            }
                            ui.add_space(6.0);
                        }
                        if ui
                            .add(theme::secondary_button(
                                RichText::new(format!("⌨ {}", self.t("return_home"))).size(13.0),
                                dark,
                            ))
                            .clicked()
                        {
                            go_home = true;
                        }
                    });
                });
            });
        if go_home {
            self.results = None;
            self.return_to_lesson();
        } else if let Some(next_path) = go_next {
            self.results = None;
            if let Some(stem) = next_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
            {
                self.current_lesson = stem;
            }
            self.page = Page::Lesson;
            self.prepare_lesson();
        } else if !open {
            self.results = None;
            self.return_to_lesson();
        }
    }

    fn return_to_lesson(&mut self) {
        let names: Vec<String> = self
            .course_paths
            .iter()
            .map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string())
            .collect();
        if names.contains(&self.last_lesson_name) {
            self.current_lesson = self.last_lesson_name.clone();
        }
        self.page = Page::Lesson;
        self.prepare_lesson();
    }

    fn results_overview(&self, ui: &mut egui::Ui, attempt: &Attempt, dark: bool) {
        theme::card_frame(dark).show(ui, |ui| {
            ui.vertical(|ui| {
                let verdict = if attempt.passed {
                    self.t("passed_tip")
                } else {
                    self.t("retry_tip")
                };
                ui.label(
                    RichText::new(verdict)
                        .size(16.0)
                        .strong()
                        .color(theme::ink(dark)),
                );
                ui.add_space(10.0);
                let facts = format!(
                    "{}: {}   {}: {}   {}: {}   {}: {}\n{}: {}   {}: {}   {}: {}   {}: {:.1}s",
                    self.t("characters"),
                    attempt.characters,
                    self.t("words"),
                    attempt.words,
                    self.t("errors"),
                    attempt.errors,
                    self.t("timeouts"),
                    attempt.timeouts,
                    self.t("fixed_characters"),
                    attempt.fixed_characters,
                    self.t("fixed_words"),
                    attempt.fixed_words,
                    self.t("backspaces"),
                    attempt.backspaces,
                    self.t("duration"),
                    attempt.duration,
                );
                ui.label(
                    RichText::new(facts)
                        .size(15.0)
                        .color(theme::muted(dark)),
                );
            });
        });
    }

    fn results_details(&mut self, ui: &mut egui::Ui, attempt: &Attempt, dark: bool) {
        // Metric dropdown (Swift chart metric picker).
        let speed_label = capitalized(&self.t("speed"));
        let acc_chars = format!("{} · {}", capitalized(&self.t("accuracy")), self.t("characters"));
        let acc_words = format!("{} · {}", capitalized(&self.t("accuracy")), self.t("words"));
        let current = match self.chart_metric {
            ChartMetric::Speed => speed_label.clone(),
            ChartMetric::AccuracyChar => acc_chars.clone(),
            ChartMetric::AccuracyWord => acc_words.clone(),
        };
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("chart_metric")
                .selected_text(current)
                .width(240.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.chart_metric, ChartMetric::Speed, speed_label);
                    ui.selectable_value(&mut self.chart_metric, ChartMetric::AccuracyChar, acc_chars);
                    ui.selectable_value(&mut self.chart_metric, ChartMetric::AccuracyWord, acc_words);
                });
        });
        ui.add_space(4.0);
        let rows = chart_rows(attempt, self.chart_metric, self.is_cpm());
        let y_label = if self.chart_metric == ChartMetric::Speed {
            self.t(self.speed_unit_key())
        } else {
            "%".to_string()
        };
        theme::card_frame(dark).show(ui, |ui| {
            if rows.is_empty() {
                ui.label(RichText::new("—").color(theme::muted(dark)));
            } else {
                ui.label(RichText::new(y_label).size(11.0).color(theme::muted(dark)));
                bar_chart(ui, &rows, dark);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").size(16.0).strong().monospace().color(theme::error(dark)));
                    ui.label(RichText::new(self.t("legend_error")).size(12.0).color(theme::muted(dark)));
                });
            }
        });
    }

    fn results_errors(&self, ui: &mut egui::Ui, attempt: &Attempt, dark: bool) {
        theme::card_frame(dark).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    for (color, key) in [
                        (theme::ink(dark), "legend_correct"),
                        (theme::muted(dark), "legend_slow"),
                        (theme::error(dark), "legend_error"),
                        (theme::error_timeout(dark), "legend_error_slow"),
                    ] {
                        ui.label(RichText::new("●").monospace().color(color));
                        ui.label(RichText::new(self.t(key)).size(13.0).color(theme::ink(dark)));
                        ui.add_space(12.0);
                    }
                });
                ui.add_space(6.0);
                // Coloured transcript: ink / muted / reds (Swift parity).
                let mut job = egui::text::LayoutJob::default();
                for (i, ch) in attempt.text.chars().enumerate() {
                    let state = attempt
                        .states
                        .get(&i.to_string())
                        .map(|s| s.as_str())
                        .unwrap_or("correct");
                    let color = match state {
                        "timeout" => theme::muted(dark),
                        "error" => theme::error(dark),
                        "error_timeout" => theme::error_timeout(dark),
                        _ => theme::ink(dark),
                    };
                    job.append(
                        &ch.to_string(),
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::monospace(18.0),
                            color,
                            ..Default::default()
                        },
                    );
                }
                ui.label(job);
            });
        });
    }
}

/// Display-only 1–5 star rating: filled brand stars vs. outline pending
/// (Swift `StarsView`, 18pt).
fn stars_view(ui: &mut egui::Ui, score: f64, dark: bool) {
    let rating = (score / 20.0).round().clamp(1.0, 5.0) as usize;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for i in 0..5 {
            let (glyph, color) = if i < rating {
                ("★", theme::brand(dark))
            } else {
                ("☆", theme::pending(dark))
            };
            ui.label(RichText::new(glyph).size(18.0).strong().color(color));
        }
    });
}

/// Label/value gauge row with a linear bar (Swift `GaugeView`).
fn gauge_view(
    ui: &mut egui::Ui,
    label: &str,
    display: &str,
    quality: f64,
    tint: Color32,
    dark: bool,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(13.0)
                .strong()
                .color(theme::muted(dark)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(display).size(14.0).strong().color(theme::ink(dark)));
        });
    });
    let frac = (quality as f32).clamp(0.0, 1.0);
    // Default width fills the available space. (Never pass INFINITY here:
    // it poisons the window sizing pass with NaN and crashes on re-show.)
    ui.add(egui::ProgressBar::new(frac).fill(tint));
    ui.add_space(2.0);
}

// ---------------------------------------------------------------------------
// Per-character chart rows (Swift `PerCharacterChart` data model)
// ---------------------------------------------------------------------------

struct ChartRow {
    label: String,
    value: f64,
    display: String,
    has_errors: bool,
}

fn pretty_char(s: &str) -> String {
    // Note: "␠" is not in egui's bundled fonts (would render as tofu),
    // so the space bar label spells the word out. "↵"/"⇥" are covered by
    // the monospace fallback and chart x-labels use the monospace family.
    match s {
        " " => "Space".to_string(),
        "\n" => "↵".to_string(),
        "\t" => "⇥".to_string(),
        _ => s.to_string(),
    }
}

fn chart_rows(attempt: &Attempt, metric: ChartMetric, is_cpm: bool) -> Vec<ChartRow> {
    match metric {
        ChartMetric::Speed => speed_rows(attempt, is_cpm),
        ChartMetric::AccuracyChar => accuracy_char_rows(attempt),
        ChartMetric::AccuracyWord => accuracy_word_rows(attempt),
    }
}

fn speed_rows(attempt: &Attempt, is_cpm: bool) -> Vec<ChartRow> {
    let mut groups: HashMap<String, Vec<f64>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut errored: HashMap<String, bool> = HashMap::new();
    for s in attempt.strokes.iter().filter(|s| !s.system_key && !s.expected.is_empty()) {
        if !groups.contains_key(&s.expected) {
            order.push(s.expected.clone());
        }
        groups.entry(s.expected.clone()).or_default().push(s.delay);
        if !s.correct {
            errored.insert(s.expected.clone(), true);
        }
    }
    // Base is CPM (60 / avg delay); WPM display divides by 5.
    let factor = if is_cpm { 1.0 } else { 0.2 };
    order
        .into_iter()
        .take(18)
        .map(|ch| {
            let delays = &groups[&ch];
            let avg = delays.iter().sum::<f64>() / delays.len().max(1) as f64;
            let value = 60.0 / avg.max(0.01) * factor;
            ChartRow {
                label: pretty_char(&ch),
                value,
                display: format!("{value:.0}"),
                has_errors: errored.get(&ch).copied().unwrap_or(false),
            }
        })
        .collect()
}

fn accuracy_char_rows(attempt: &Attempt) -> Vec<ChartRow> {
    let mut groups: HashMap<String, (usize, usize)> = HashMap::new();
    for s in attempt.strokes.iter().filter(|s| !s.system_key && !s.expected.is_empty()) {
        let e = groups.entry(s.expected.clone()).or_insert((0, 0));
        e.1 += 1;
        if s.correct {
            e.0 += 1;
        }
    }
    let mut rows: Vec<ChartRow> = groups
        .into_iter()
        .map(|(ch, (correct, total))| {
            let value = correct as f64 / total.max(1) as f64 * 100.0;
            ChartRow {
                label: pretty_char(&ch),
                value,
                display: format!("{value:.0}%"),
                has_errors: correct < total,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap());
    rows.truncate(18);
    rows
}

fn accuracy_word_rows(attempt: &Attempt) -> Vec<ChartRow> {
    let chars: Vec<char> = attempt.text.chars().collect();
    let mut totals: HashMap<String, (usize, usize)> = HashMap::new();
    let mut word = String::new();
    let mut indices: Vec<usize> = Vec::new();
    let flush = |word: &mut String, indices: &mut Vec<usize>, totals: &mut HashMap<String, (usize, usize)>| {
        if word.is_empty() {
            return;
        }
        let bad = indices
            .iter()
            .filter(|i| {
                matches!(
                    attempt.states.get(&i.to_string()).map(|s| s.as_str()),
                    Some("error" | "error_timeout")
                )
            })
            .count();
        let e = totals.entry(word.clone()).or_insert((0, 0));
        e.0 += indices.len();
        e.1 += bad;
        word.clear();
        indices.clear();
    };
    for (offset, ch) in chars.iter().enumerate() {
        if ch.is_whitespace() {
            flush(&mut word, &mut indices, &mut totals);
        } else {
            word.push(*ch);
            indices.push(offset);
        }
    }
    flush(&mut word, &mut indices, &mut totals);
    let mut rows: Vec<ChartRow> = totals
        .into_iter()
        .map(|(text, (total, bad))| {
            let value = (total - bad) as f64 / total.max(1) as f64 * 100.0;
            let label = if text.chars().count() > 12 {
                format!("{}…", text.chars().take(12).collect::<String>())
            } else {
                text
            };
            ChartRow {
                label,
                value,
                display: format!("{value:.0}%"),
                has_errors: bad > 0,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap());
    rows.truncate(18);
    rows
}

/// Vertical bar chart (Swift Charts `BarMark`): value annotations on top,
/// errors in red, hidden legend, Y-axis label.
/// Fixed width of one chart column. Bars keep this slot and the box scrolls
/// sideways when there are too many (never squeezed into overlap).
const CHART_SLOT: f32 = 56.0;

/// Vertical bar chart (Swift Charts `BarMark`): value annotations on top,
/// errors in red, hidden legend. Slots are fixed-width; the caller wraps
/// this in a sideways scroll area when there are many bars.
fn bar_chart(ui: &mut egui::Ui, rows: &[ChartRow], dark: bool) {
    let max = rows.iter().map(|r| r.value).fold(1.0, f64::max);
    let height = 240.0;
    let n = rows.len().max(1) as f32;
    // Spread across the viewport when few, fixed slots when many.
    let slot = (ui.available_width() / n).max(CHART_SLOT);
    let content_w = (slot * n).max(ui.available_width());
    egui::ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(content_w, height + 44.0),
                egui::Sense::hover(),
            );
            let plot = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + 20.0),
                egui::vec2(rect.width(), height),
            );
            let bar_w = (slot - 16.0).clamp(8.0, 44.0);
            for (i, row) in rows.iter().enumerate() {
                let cx = plot.left() + slot * i as f32 + slot / 2.0;
                let h = (row.value / max) as f32 * plot.height();
                let bar = egui::Rect::from_min_size(
                    egui::pos2(cx - bar_w / 2.0, plot.bottom() - h),
                    egui::vec2(bar_w, h.max(2.0)),
                );
                let fill = if row.has_errors {
                    theme::error(dark)
                } else {
                    theme::ink(dark)
                };
                ui.painter().rect_filled(bar, 4.0, fill);
                // Value annotation on top.
                ui.painter().text(
                    egui::pos2(cx, bar.top() - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    row.display.clone(),
                    egui::FontId::new(11.0, egui::FontFamily::Proportional),
                    if row.has_errors {
                        theme::error(dark)
                    } else {
                        theme::muted(dark)
                    },
                );
                // X label (monospace so ↵/⇥ resolve through the Hack fallback).
                ui.painter().text(
                    egui::pos2(cx, plot.bottom() + 4.0),
                    egui::Align2::CENTER_TOP,
                    row.label.clone(),
                    egui::FontId::monospace(11.0),
                    theme::muted(dark),
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harness_app() -> (tempfile::TempDir, BitTypingApp) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let settings = Settings::default();
        let audio = ClickPlayer::new(&root.join("sounds"));
        audio.set_enabled(false);
        let app = BitTypingApp {
            root: root.clone(),
            opt_language: settings.language.clone(),
            opt_keyboard: settings.keyboard_layout.clone(),
            opt_typo: settings.typo_mode.clone(),
            opt_goal_wpm: trim_num(settings.goal_wpm),
            opt_goal_accuracy: trim_num(settings.goal_accuracy),
            opt_timeout: trim_num(settings.timeout_seconds),
            opt_timed: trim_num(settings.timed_minutes),
            opt_backspace: settings.backspace,
            opt_auto_start: settings.auto_start,
            opt_sound: false,
            opt_metronome: settings.metronome,
            opt_error: String::new(),
            settings,
            i18n: I18n::load(&root.join("lang")),
            layouts: BTreeMap::new(),
            course_paths: Vec::new(),
            custom: BTreeSet::new(),
            history: Vec::new(),
            session: Session::new("", ""),
            current_lesson: String::new(),
            last_lesson_name: String::new(),
            page: Page::Lesson,
            sidebar_collapsed: false,
            text_shift: 0.0,
            text_anim_from: 0.0,
            text_anim_t0: Instant::now(),
            text_animating: false,
            show_options: false,
            show_about: false,
            show_setup: false,
            results: None,
            results_tab: ResultsTab::Overview,
            chart_metric: ChartMetric::Speed,
            well_done_until: None,
            pending_results: None,
            editor_name: String::new(),
            editor_text: String::new(),
            stats_selected: None,
            confirm_clear_history: false,
            confirm_replace: None,
            pending_imports: Vec::new(),
            audio,
            clock: SystemClock::default(),
            save_error: String::new(),
            brand_icon: None,
            avatar_ms: None,
            avatar_cacto: None,
        };
        (tmp, app)
    }

    fn headless_ctx() -> (egui::Context, egui::RawInput) {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        (ctx, raw)
    }

    #[test]
    fn results_flow_renders_headless() {
        let (_tmp, mut app) = harness_app();
        app.current_lesson = "01".to_string();
        app.session = Session::new("01", "ab cd ef gh ij");
        app.start_session();
        // Include an error + a backspace so error/timeout paths are covered.
        app.handle_char_input('x');
        app.session.handle_backspace(&app.settings, &app.clock);
        let chars: Vec<char> = app.session.text.clone();
        while app.session.index < chars.len() {
            let expected = chars[app.session.index];
            app.handle_char_input(expected);
        }
        assert!(!app.session.running);
        assert!(app.pending_results.is_some());
        app.results_tab = ResultsTab::Overview;
        app.chart_metric = ChartMetric::Speed;
        app.results = app.pending_results.take();
        assert!(app.results.is_some());

        let (ctx, _) = headless_ctx();
        // NOTE: one shared context across frames, like the real app where the
        // sheet re-shows every frame until dismissed. (A fresh ctx per frame
        // would hide sizing-pass state bugs such as NaN window rects.)
        for tab in [ResultsTab::Overview, ResultsTab::Details, ResultsTab::Errors] {
            for metric in [
                ChartMetric::Speed,
                ChartMetric::AccuracyChar,
                ChartMetric::AccuracyWord,
            ] {
                app.results_tab = tab;
                app.chart_metric = metric;
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 850.0),
                    )),
                    ..Default::default()
                };
                let full = ctx.run(raw, |ctx| app.results_window(ctx));
                let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
            }
        }
        // Re-show the same sheet several frames in a row (steady state).
        for _ in 0..5 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 850.0),
                )),
                ..Default::default()
            };
            let full = ctx.run(raw, |ctx| app.results_window(ctx));
            let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        }
        // Splash + main pages render headless too.
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        let full = ctx.run(raw, |ctx| app.well_done_overlay(ctx));
        let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        app.results = None;
        for page in [Page::Lesson, Page::Stats, Page::Editor] {
            app.page = page;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 850.0),
                )),
                ..Default::default()
            };
            let full = ctx.run(raw, |ctx| {
                app.toolbar(ctx);
                app.side_bar(ctx);
                egui::CentralPanel::default().show(ctx, |ui| match app.page {
                    Page::Lesson => app.lesson_page(ui),
                    Page::Stats => app.stats_page(ui),
                    Page::Editor => app.editor_page(ui),
                });
            });
            let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        }
    }

    /// About sheet renders headless without panicking, both with the
    /// initials fallback and with the real bundled photos, across frames.
    /// The sheet is fixed-size (620x600), so content can never balloon.
    #[test]
    fn about_window_renders_headless() {
        let (_tmp, mut app) = harness_app();
        let ctx = egui::Context::default();
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        // Fallback path: no textures bundled in the harness.
        app.show_about = true;
        for _ in 0..3 {
            let full = ctx.run(frame(), |ctx| app.about_window(ctx));
            let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        }
        assert!(app.show_about);
        // Photo path: decode the real embedded assets.
        app.brand_icon = load_bundled_texture(&ctx, "test-brand", "assets/favico.png", 192);
        app.avatar_ms =
            load_bundled_texture(&ctx, "test-ms", "assets/micr0softstore.png", 192);
        app.avatar_cacto =
            load_bundled_texture(&ctx, "test-cacto", "assets/cacto.tsx.png", 192);
        assert!(app.brand_icon.is_some(), "favico.png must decode");
        assert!(app.avatar_ms.is_some(), "micr0softstore.png must decode");
        assert!(app.avatar_cacto.is_some(), "cacto.tsx.png must decode");
        app.show_about = true;
        for _ in 0..3 {
            let full = ctx.run(frame(), |ctx| app.about_window(ctx));
            let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        }
        assert!(app.show_about);
    }

    /// Keyboard metrics track the window width like Swift's GeometryReader:
    /// capped at a 920pt board, shrinking keys/fonts on narrow windows,
    /// and every hardware row filling the board edge to edge.
    #[test]
    fn keyboard_metrics_resize() {
        // Cap: wider windows keep the 920pt board (metrics identical).
        assert_eq!(keyboard_board_width(1400.0), 920.0);
        let wide_a = keyboard_metrics(keyboard_board_width(1200.0));
        let wide_b = keyboard_board_width(2000.0);
        let wide_b = keyboard_metrics(wide_b);
        assert_eq!(wide_a.key_h, wide_b.key_h);
        assert_eq!(wide_a.font, wide_b.font);
        // Shrink: narrower windows get smaller keys.
        let narrow = keyboard_metrics(keyboard_board_width(600.0));
        let mid = keyboard_metrics(keyboard_board_width(900.0));
        assert!(narrow.key_h < mid.key_h);
        assert!(narrow.font <= mid.font);
        // Floors/ceilings hold across a wide range of sizes.
        for w in [380.0, 500.0, 800.0, 920.0, 1200.0, 1920.0] {
            let m = keyboard_metrics(keyboard_board_width(w));
            assert!((30.0..=42.0).contains(&m.key_h), "key_h {w}");
            assert!((10.0..=16.0).contains(&m.font), "font {w}");
            assert!((9.0..=12.0).contains(&m.mod_font), "mod {w}");
        }
        // Every row of a real-shaped layout fills the board exactly.
        let layout = KeyboardLayout {
            name: String::new(),
            rows: vec![
                (0..13).map(|i| i.to_string()).collect(),
                (0..12).map(|i| i.to_string()).collect(),
                (0..12).map(|i| i.to_string()).collect(),
                (0..11).map(|i| i.to_string()).collect(),
            ],
            space_label: "SPACE".to_string(),
        };
        for w in [500.0, 900.0, 1400.0] {
            let m = keyboard_metrics(keyboard_board_width(w));
            for row in board_rows(&layout) {
                let units: f32 = row.iter().map(|s| s.units).sum();
                assert!((units - 15.0).abs() < 0.01, "row sums to 15u");
                let total = m.row_width(units, row.len());
                assert!(
                    (total - m.board).abs() < 0.05,
                    "row fills board at {w}: {total} vs {}",
                    m.board
                );
            }
        }
    }

    /// Pinned viewport: the shift always lands the active character exactly
    /// in the middle, at any width (Swift `scrollTo(anchor: .center)`).
    #[test]
    fn pinned_shift_centers_active() {
        for (cx, w) in [(20.0, 1252.0), (2661.5, 1252.0), (0.0, 800.0), (5000.0, 300.0)] {
            let shift = pinned_shift(cx, w);
            assert!((cx - shift - w * 0.5).abs() < 1e-4, "cx {cx} w {w}");
        }
    }

    /// Pop pulse: starts at 1.18x, eases down, rests exactly at 1.0.
    #[test]
    fn pop_scale_curve() {
        assert!((pop_scale(0.0, true) - 1.18).abs() < 1e-4);
        let mid = pop_scale(0.07, true);
        assert!(mid > 1.0 && mid < 1.18, "easing {mid}");
        assert_eq!(pop_scale(1.0, true), 1.0);
        assert_eq!(pop_scale(0.0, false), 1.0);
    }

    /// Lesson focus guard: button focus is dropped so Space/Enter always
    /// types; modals and text fields keep their focus.
    #[test]
    fn lesson_drops_stolen_focus() {
        let (_tmp, app) = harness_app();
        let ctx = egui::Context::default();
        let id = egui::Id::new("start-button");
        ctx.memory_mut(|m| m.request_focus(id));
        assert!(ctx.memory(|m| m.focused()).is_some());
        app.drop_stolen_focus(&ctx);
        assert!(
            ctx.memory(|m| m.focused()).is_none(),
            "button focus must be dropped on the lesson page"
        );
    }

    /// End-to-end freeze regression: with the Start button focused (as after
    /// clicking it), a keystroke must still reach the lesson — previously
    /// `wants_keyboard_input` (true for ANY focused widget) swallowed all
    /// typing forever, and Space additionally fake-clicked the button.
    #[test]
    fn lesson_types_despite_button_focus() {
        let (_tmp, mut app) = harness_app();
        app.current_lesson = "01".to_string();
        app.session = Session::new("01", "keke");
        app.start_session();
        let ctx = egui::Context::default();
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("start-button")));
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            events: vec![egui::Event::Text("k".to_string())],
            ..Default::default()
        };
        ctx.run(raw, |ctx| {
            app.drop_stolen_focus(ctx);
            app.handle_input(ctx);
        });
        assert_eq!(app.session.index, 1, "keystroke must type, not freeze");
        assert!(
            ctx.memory(|m| m.focused()).is_none(),
            "button focus must be gone after the frame"
        );
    }

    /// Character-switch glide: typing starts the animation, frames ease the
    /// shift toward the new center, and it lands exactly and stays put.
    #[test]
    fn lesson_glide_animates_and_settles() {
        let (_tmp, mut app) = harness_app();
        app.current_lesson = "01".to_string();
        app.session = Session::new("01", &"ab cd ef gh ij kl mn ".repeat(20));
        app.start_session();
        let ctx = egui::Context::default();
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        let render = |app: &mut BitTypingApp, ctx: &egui::Context| {
            let full = ctx.run(frame(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.lesson_page(ui));
            });
            let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        };
        // Settle at lesson start, then switch one character.
        render(&mut app, &ctx);
        render(&mut app, &ctx);
        assert!(!app.text_animating);
        let before = app.text_shift;
        let expected = app.session.text[app.session.index];
        app.handle_char_input(expected);
        assert!(app.text_animating, "keystroke must start the glide");
        // Immediate frame: still near the origin (ease-out start).
        render(&mut app, &ctx);
        assert!(app.text_animating, "glide must span frames");
        // Past 0.12s: landed exactly and released.
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(30));
            render(&mut app, &ctx);
        }
        assert!(!app.text_animating, "glide must finish");
        assert!(
            (app.text_shift - before).abs() > 5.0,
            "shift must have moved to the new character"
        );
        let landed = app.text_shift;
        render(&mut app, &ctx);
        render(&mut app, &ctx);
        assert!(
            (app.text_shift - landed).abs() < 1e-3,
            "settled shift must be stable"
        );
    }

    /// Human-like typing simulation: 30 keystrokes at ~8 chars/sec with a
    /// frame after each. EVERY switch must start the glide (never silently
    /// stop animating mid-lesson), and the shift must keep tracking the
    /// active character to the end.
    #[test]
    fn lesson_glide_fires_every_keystroke() {
        let (_tmp, mut app) = harness_app();
        app.current_lesson = "01".to_string();
        app.session = Session::new("01", &"ab cd ef gh ij kl mn ".repeat(20));
        app.start_session();
        let ctx = egui::Context::default();
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        let render = |app: &mut BitTypingApp, ctx: &egui::Context| {
            let full = ctx.run(frame(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.lesson_page(ui));
            });
            let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        };
        render(&mut app, &ctx);
        let mut switches = 0;
        for _ in 0..30 {
            let before = app.session.index;
            let expected = app.session.text[before];
            app.handle_char_input(expected);
            assert_eq!(app.session.index, before + 1, "keystroke must advance");
            assert!(
                app.text_animating,
                "switch #{switches} (index {before}) must start the glide"
            );
            switches += 1;
            render(&mut app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
        assert_eq!(switches, 30);
        // Settle fully, then the shift must sit exactly on target.
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(30));
            render(&mut app, &ctx);
        }
        assert!(!app.text_animating, "glide must finish after typing stops");
    }

    /// Lesson chrome aligns at fullscreen size (headless shape analysis):
    /// the 920pt keyboard board is centered in the content column, and the
    /// active-character outline is a compact cell (~60pt, was 512pt when the
    /// `Frame` widget stretched to the row height), centered horizontally.
    #[test]
    fn lesson_chrome_aligns_fullscreen() {
        let (_tmp, mut app) = harness_app();
        app.current_lesson = "01".to_string();
        app.session = Session::new("01", &"ab cd ef gh ij kl mn ".repeat(20));
        // The harness ships no layouts; inject a Hungarian-shaped board.
        app.layouts.insert(
            "HU-qwertz".to_string(),
            KeyboardLayout {
                name: "HU".to_string(),
                rows: vec![
                    (0..13).map(|i| i.to_string()).collect(),
                    (0..12).map(|i| i.to_string()).collect(),
                    (0..12).map(|i| i.to_string()).collect(),
                    (0..11).map(|i| i.to_string()).collect(),
                ],
                space_label: "SPACE".to_string(),
            },
        );
        app.settings.keyboard_layout = "HU-qwertz".to_string();
        app.start_session();
        for _ in 0..10 {
            let expected = app.session.text[app.session.index];
            app.handle_char_input(expected);
        }
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0),
            )),
            ..Default::default()
        };
        let mut full = None;
        // Settle the character-switch glide before measuring (≈0.12s).
        for _ in 0..6 {
            full = Some(ctx.run(frame(), |ctx| {
                app.toolbar(ctx);
                app.side_bar(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.lesson_page(ui));
            }));
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let full = full.unwrap();
        let _ = ctx.tessellate(full.shapes.clone(), full.pixels_per_point);

        // Keycap fills (finger tints + surfaces + ink active key).
        let mut fills = vec![
            theme::keycap(dark),
            theme::panel2(dark),
            theme::ink(dark),
        ];
        for zone in [
            FingerZone::Pinky,
            FingerZone::Ring,
            FingerZone::Middle,
            FingerZone::Index,
        ] {
            fills.push(theme::finger_tint(zone, dark));
        }
        let mut left = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        let mut active_box: Option<egui::Rect> = None;
        for clipped in &full.shapes {
            if let egui::Shape::Rect(rs) = &clipped.shape {
                let h = rs.rect.height();
                if rs.stroke.color == theme::brand(dark) && !rs.stroke.is_empty() {
                    active_box = Some(rs.rect);
                    continue;
                }
                // Keycaps: right of the sidebar, below the toolbar, key-sized.
                if fills.contains(&rs.fill)
                    && (20.0..=50.0).contains(&h)
                    && rs.rect.left() > 300.0
                    && rs.rect.top() > 400.0
                {
                    left = left.min(rs.rect.left());
                    right = right.max(rs.rect.right());
                }
            }
        }
        // Column: sidebar 204 + 20 padding; board 920 centered in 1676.
        assert!(left.is_finite() && right.is_finite(), "no keycaps found");
        assert!(
            (left - 602.0).abs() < 12.0,
            "board left {left}, want ~602 (centered 920 board)"
        );
        assert!(
            (right - 1522.0).abs() < 12.0,
            "board right {right}, want ~1522"
        );
        let cell = active_box.expect("active outline missing");
        assert!(
            cell.height() < 110.0,
            "active box too tall: {} (was 512)",
            cell.height()
        );
        assert!(cell.height() > 30.0, "active box collapsed?");
        assert!(
            (cell.center().x - 1062.0).abs() < 12.0,
            "active not centered: {}",
            cell.center().x
        );
    }

    /// Renders the full chrome headless at 1280x850 and returns the painted
    /// rect shapes, so tests can assert on the real layout geometry.
    #[cfg(test)]
    fn chrome_shapes(app: &mut BitTypingApp) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        let mut full = None;
        for _ in 0..2 {
            full = Some(ctx.run(frame(), |ctx| {
                app.toolbar(ctx);
                app.side_bar(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.lesson_page(ui));
            }));
        }
        let full = full.unwrap();
        let _ = ctx.tessellate(full.shapes.clone(), full.pixels_per_point);
        full.shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    /// Sidebar is an inset outlined panel (Swift sidebar); collapsing it via
    /// the flag removes the panel while the toolbar toggle stays usable.
    #[test]
    fn chrome_sidebar_panel() {
        let (_tmp, mut app) = harness_app();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        let panel_rects = |shapes: &[egui::Shape]| {
            shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Rect(rs)
                        if rs.stroke.color == theme::border(dark)
                            && rs.corner_radius == egui::CornerRadius::same(12)
                            && rs.rect.left() < 30.0
                            && rs.rect.height() > 400.0 =>
                    {
                        Some(rs.rect)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Expanded: outlined panel on the left.
        let shapes = chrome_shapes(&mut app);
        let panels = panel_rects(&shapes);
        assert_eq!(panels.len(), 1, "sidebar panel missing: {panels:?}");
        assert!(
            (panels[0].width() - 184.0).abs() < 12.0,
            "panel width {} (204 sidebar minus 2x10 inset)",
            panels[0].width()
        );
        // Collapsed: panel gone, rest of the chrome still renders.
        app.sidebar_collapsed = true;
        let shapes = chrome_shapes(&mut app);
        assert!(
            panel_rects(&shapes).is_empty(),
            "sidebar panel must vanish when collapsed"
        );
    }

    /// Lesson toolbar matches the Swift bar: lesson picker, blue circle
    /// start/pause, and one outlined stats pill (TIME/ACCURACY/SPEED).
    #[test]
    fn chrome_lesson_toolbar_pills() {
        let (_tmp, mut app) = harness_app();
        // A selectable lesson keeps Start enabled (brand circle painted).
        app.course_paths = vec![std::path::PathBuf::from("01.txt")];
        app.current_lesson = "01".to_string();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        let shapes = chrome_shapes(&mut app);
        // Blue circle: brand fill, fully round, toolbar band.
        let circles = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(rs)
                    if rs.fill == theme::brand(dark)
                        && rs.corner_radius == egui::CornerRadius::same(22)
                        && (36.0..=52.0).contains(&rs.rect.height())
                        && rs.rect.top() < 120.0 =>
                {
                    Some(rs.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(circles.len(), 1, "blue circle button missing");
        // Stats pill: bordered, pill radius, wide enough for the trio.
        let pills = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(rs)
                    if rs.stroke.color == theme::border(dark)
                        && rs.corner_radius == egui::CornerRadius::same(16)
                        && (24.0..=60.0).contains(&rs.rect.height())
                        && rs.rect.width() > 200.0
                        && rs.rect.top() < 120.0 =>
                {
                    Some(rs.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pills.len(), 1, "outlined stats pill missing");
        // Order along the bar: picker < circle < stats pill.
        assert!(
            circles[0].left() > 200.0,
            "circle should sit after the picker pill"
        );
        assert!(
            pills[0].left() > circles[0].right(),
            "stats pill should sit after the circle"
        );
        // The pill is centered in the full 1280 bar.
        let center = (pills[0].left() + pills[0].right()) / 2.0;
        assert!(
            (center - 640.0).abs() < 15.0,
            "stats pill not centered: {center}"
        );
    }

    /// Toolbar height is identical on all pages (exact 52px), so the sidebar
    /// panel below it never grows or shrinks when switching tabs.
    #[test]
    fn chrome_toolbar_height_uniform() {
        let (_tmp, mut app) = harness_app();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        let mut tops = Vec::new();
        for page in [Page::Lesson, Page::Stats, Page::Editor] {
            app.page = page;
            let mut panel_top = None;
            for _ in 0..2 {
                let full = ctx.run(frame(), |ctx| {
                    app.toolbar(ctx);
                    app.side_bar(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| match app.page {
                        Page::Lesson => app.lesson_page(ui),
                        Page::Stats => app.stats_page(ui),
                        Page::Editor => app.editor_page(ui),
                    });
                });
                let _ = ctx.tessellate(full.shapes.clone(), full.pixels_per_point);
                for clipped in &full.shapes {
                    if let egui::Shape::Rect(rs) = &clipped.shape {
                        if rs.stroke.color == theme::border(dark)
                            && rs.corner_radius == egui::CornerRadius::same(12)
                            && rs.rect.left() < 30.0
                            && rs.rect.height() > 400.0
                        {
                            panel_top = Some(rs.rect.top());
                        }
                    }
                }
            }
            tops.push(panel_top.expect("sidebar panel missing"));
        }
        // 52px toolbar + 10px inset on every page.
        for top in &tops {
            assert!((top - 62.0).abs() < 8.0, "sidebar top {top}, want ~62");
        }
        assert!(
            tops.windows(2).all(|w| (w[0] - w[1]).abs() < 2.0),
            "sidebar moves between pages: {tops:?}"
        );
    }

    /// Imported files land in the course list; editor imports also jump to
    /// the editor with the new lesson loaded. (The native picker itself is
    /// exercised by hand — it needs a display server.)
    #[test]
    fn import_pending_files() {
        let (_tmp, mut app) = harness_app();
        let courses = crate::paths::courses_dir(&app.root);
        std::fs::create_dir_all(&courses).unwrap();
        let src = app.root.join("incoming.txt");
        std::fs::write(&src, "hello import").unwrap();
        // Plain import: file copied, lesson list refreshed, page kept.
        app.pending_imports = vec![src.clone()];
        app.import_pending(false);
        assert!(courses.join("incoming.txt").is_file());
        assert!(app.custom.contains("incoming.txt"));
        assert_eq!(app.page, Page::Lesson);
        assert_eq!(app.current_lesson, "incoming");
        // Editor import: jumps to the editor with the text loaded.
        std::fs::write(&src, "second version").unwrap();
        app.pending_imports = vec![src.clone()];
        app.page = Page::Lesson;
        app.import_pending(true);
        assert_eq!(app.page, Page::Editor);
        assert_eq!(app.editor_name, "incoming");
        assert_eq!(app.editor_text, "second version");
        // Unreadable file: surfaced as an error, nothing imported.
        app.save_error.clear();
        app.pending_imports = vec![app.root.join("missing.txt")];
        app.import_pending(false);
        assert!(!app.save_error.is_empty());
    }

    /// Details chart lays out one fixed slot per character and scrolls
    /// sideways when there are too many (never squeezed/overlapping):
    /// 18 distinct characters must yield 18 uniform bars spanning wider
    /// than the sheet viewport.
    #[test]
    fn results_chart_scrolls_sideways() {
        let (_tmp, mut app) = harness_app();
        app.current_lesson = "01".to_string();
        app.session = Session::new("01", "abcdefghijklmnopqr");
        app.start_session();
        while app.session.index < app.session.text.len() {
            let expected = app.session.text[app.session.index];
            app.handle_char_input(expected);
        }
        assert!(app.pending_results.is_some());
        app.results = app.pending_results.take();
        app.results_tab = ResultsTab::Details;
        app.chart_metric = ChartMetric::Speed;
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        // egui sheets fade in on wall-clock time, which is frozen headless:
        // advance `time` so the window actually appears.
        let frame = |t: f64| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            time: Some(t),
            ..Default::default()
        };
        let mut full = None;
        for t in [0.0, 0.4, 0.8, 1.2] {
            full = Some(ctx.run(frame(t), |ctx| app.results_window(ctx)));
        }
        let full = full.unwrap();
        let _ = ctx.tessellate(full.shapes.clone(), full.pixels_per_point);
        // Chart bars: ink-filled, tall, narrow (gauges are short/full-width).
        let mut bars = full
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rs)
                    if rs.fill == theme::ink(dark)
                        && rs.rect.height() > 60.0
                        && rs.rect.width() < 60.0 =>
                {
                    Some(rs.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        bars.sort_by(|a, b| a.left().partial_cmp(&b.left()).unwrap());
        assert_eq!(bars.len(), 18, "one bar per character");
        let widths_ok = bars
            .iter()
            .all(|r| (r.width() - bars[0].width()).abs() < 2.0);
        assert!(widths_ok, "uniform bar widths");
        let span = bars.last().unwrap().right() - bars.first().unwrap().left();
        assert!(
            span > 750.0,
            "chart must overflow sideways with 18 bars, span {span}"
        );
    }

    /// Stats table fills the whole box with pill rows (Swift empty state):
    /// empty history still shows a full screen of pills; with data, odd
    /// rows stay uncolored (stripes) between the filled pills.
    #[test]
    fn stats_table_fillers_and_stripes() {
        fn pill_tops(shapes: &[egui::Shape], dark: bool) -> Vec<f32> {
            let mut tops = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Rect(rs)
                        if rs.fill == theme::panel2(dark)
                            && rs.corner_radius == egui::CornerRadius::same(8)
                            && (28.0..=36.0).contains(&rs.rect.height())
                            && rs.rect.left() > 100.0 =>
                    {
                        Some(rs.rect.top())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
            tops
        }
        // Empty history: a full box of filler pills.
        let (_tmp, mut app) = harness_app();
        app.page = Page::Stats;
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let dark = ctx.style().visuals.dark_mode;
        let frame = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 850.0),
            )),
            ..Default::default()
        };
        let render = |app: &mut BitTypingApp, ctx: &egui::Context| {
            let mut full = None;
            for _ in 0..2 {
                full = Some(ctx.run(frame(), |ctx| {
                    app.toolbar(ctx);
                    app.side_bar(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| app.stats_page(ui));
                }));
            }
            let full = full.unwrap();
            let _ = ctx.tessellate(full.shapes.clone(), full.pixels_per_point);
            full.shapes
                .into_iter()
                .map(|clipped| clipped.shape)
                .collect::<Vec<_>>()
        };
        let tops = pill_tops(&render(&mut app, &ctx), dark);
        assert!(tops.len() >= 10, "fillers must fill the box: {}", tops.len());
        // Three attempts: rows 0 and 2 filled, row 1 uncolored (stripe),
        // then filled fillers — exactly one double gap in the pill rhythm.
        for text in ["abc", "defg", "hi jk"] {
            app.session = Session::new("s", text);
            app.start_session();
            while app.session.index < app.session.text.len() {
                let expected = app.session.text[app.session.index];
                app.handle_char_input(expected);
            }
        }
        assert_eq!(app.history.len(), 3);
        let tops = pill_tops(&render(&mut app, &ctx), dark);
        assert!(tops.len() >= 10, "box stays full with data");
        let gaps = tops
            .windows(2)
            .map(|w| w[1] - w[0])
            .collect::<Vec<_>>();
        assert!(
            gaps.iter().any(|g| *g > 80.0),
            "stripe gap missing (all rows filled?): {gaps:?}"
        );
        assert!(
            gaps.iter().any(|g| *g < 60.0),
            "no normal rhythm found: {gaps:?}"
        );
    }

    /// Lesson page renders headless without panicking at default and
    /// fullscreen sizes, before typing and deep into a lesson: the pinned
    /// row, keyboard, legend and footer all track the window size.
    #[test]
    fn lesson_page_renders_at_sizes() {
        let (_tmp, mut app) = harness_app();
        app.current_lesson = "01".to_string();
        app.session = Session::new("01", &"ab cd ef gh ij kl mn ".repeat(20));
        app.start_session();
        for _ in 0..120 {
            let expected = app.session.text[app.session.index];
            app.handle_char_input(expected);
        }
        for (w, h) in [(1280.0, 850.0), (1920.0, 1080.0), (1050.0, 760.0)] {
            let ctx = egui::Context::default();
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, h),
                )),
                ..Default::default()
            };
            let full = ctx.run(raw, |ctx| {
                app.toolbar(ctx);
                app.side_bar(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.lesson_page(ui));
            });
            let _ = ctx.tessellate(full.shapes, full.pixels_per_point);
        }
    }
}
