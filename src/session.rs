//! Typing engine: faithful port of `Session` + key handling in `main.py`.
//!
//! Time is injected (`now` seconds from an [`Clock`]) so the engine is fully
//! unit-testable without sleeping.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::history::{Attempt, Stroke};
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharState {
    Pending,
    Correct,
    Timeout,
    Error,
    ErrorTimeout,
}

impl CharState {
    pub fn as_str(self) -> &'static str {
        match self {
            CharState::Pending => "pending",
            CharState::Correct => "correct",
            CharState::Timeout => "timeout",
            CharState::Error => "error",
            CharState::ErrorTimeout => "error_timeout",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "correct" => CharState::Correct,
            "timeout" => CharState::Timeout,
            "error" => CharState::Error,
            "error_timeout" => CharState::ErrorTimeout,
            _ => CharState::Pending,
        }
    }
}

/// Wall + monotonic clock abstraction (production uses [`SystemClock`]).
pub trait Clock {
    fn now(&self) -> f64;
    fn wall_iso(&self) -> String;
}

#[derive(Debug, Clone)]
pub struct SystemClock {
    start: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self { start: Instant::now() }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn wall_iso(&self) -> String {
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub lesson: String,
    pub text: Vec<char>,
    pub index: usize,
    pub started_wall: String,
    pub started: Option<f64>,
    pub paused_at: f64,
    pub paused_total: f64,
    pub last_key: f64,
    pub running: bool,
    pub paused: bool,
    pub errors: usize,
    pub timeouts: usize,
    pub backspaces: usize,
    pub typed_keys: usize,
    pub correct_keys: usize,
    pub fixed_indices: HashSet<usize>,
    pub pending_wrong: bool,
    pub states: HashMap<usize, CharState>,
    pub strokes: Vec<Stroke>,
    pub tip: Tip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tip {
    Initial,
    Ready,
    Typing,
    Resumed,
    Paused,
    BackspaceDisabled,
    TypeCorrect,
    PressBackspace,
}

impl Session {
    pub fn new(lesson: &str, text: &str) -> Self {
        Self {
            lesson: lesson.to_string(),
            text: text.chars().collect(),
            index: 0,
            started_wall: String::new(),
            started: None,
            paused_at: 0.0,
            paused_total: 0.0,
            last_key: 0.0,
            running: false,
            paused: false,
            errors: 0,
            timeouts: 0,
            backspaces: 0,
            typed_keys: 0,
            correct_keys: 0,
            fixed_indices: HashSet::new(),
            pending_wrong: false,
            states: HashMap::new(),
            strokes: Vec::new(),
            tip: Tip::Ready,
        }
    }

    pub fn elapsed(&self, now: f64) -> f64 {
        match self.started {
            None => 0.0,
            Some(started) => {
                let end = if self.paused { self.paused_at } else { now };
                (end - started - self.paused_total).max(0.0)
            }
        }
    }

    pub fn start(&mut self, clock: &dyn Clock) {
        let now = clock.now();
        self.started = Some(now);
        self.last_key = now;
        self.started_wall = clock.wall_iso();
        self.running = true;
        self.paused = false;
        self.tip = Tip::Typing;
    }

    pub fn toggle(&mut self, clock: &dyn Clock) {
        if !self.running {
            self.start(clock);
        } else if self.paused {
            let now = clock.now();
            self.paused_total += now - self.paused_at;
            self.paused = false;
            self.tip = Tip::Resumed;
        } else {
            self.paused_at = clock.now();
            self.paused = true;
            self.tip = Tip::Paused;
        }
    }

    pub fn live_metrics(&self, now: f64) -> LiveMetrics {
        let elapsed = self.elapsed(now);
        let wpm = if elapsed > 0.0 {
            self.correct_keys as f64 / 5.0 / (elapsed / 60.0)
        } else {
            0.0
        };
        let accuracy = if self.typed_keys > 0 {
            self.correct_keys as f64 / self.typed_keys as f64 * 100.0
        } else {
            100.0
        };
        LiveMetrics { elapsed, wpm, accuracy }
    }

    pub fn record_system(&mut self, clock: &dyn Clock, key: &str) {
        let now = clock.now();
        let delay = now - self.last_key;
        self.last_key = now;
        self.strokes.push(Stroke {
            expected: String::new(),
            actual: key.to_string(),
            timestamp: now,
            delay,
            correct: true,
            timed_out: false,
            index: self.index,
            system_key: true,
        });
    }

    /// Returns `true` when the lesson just completed.
    pub fn handle_backspace(&mut self, settings: &Settings, clock: &dyn Clock) -> bool {
        if !settings.backspace {
            self.tip = Tip::BackspaceDisabled;
            return false;
        }
        self.record_system(clock, "Backspace");
        self.backspaces += 1;
        let current_is_error = matches!(
            self.states.get(&self.index),
            Some(CharState::Error | CharState::ErrorTimeout)
        );
        if self.pending_wrong || current_is_error {
            self.pending_wrong = false;
            self.states.remove(&self.index);
            self.tip = Tip::TypeCorrect;
        } else if self.index > 0 {
            self.index -= 1;
            self.fixed_indices.insert(self.index);
            self.states.remove(&self.index);
            self.pending_wrong = false;
        }
        false
    }

    /// Returns `true` when the lesson just completed.
    pub fn handle_character(
        &mut self,
        settings: &Settings,
        clock: &dyn Clock,
        actual: char,
    ) -> CharacterOutcome {
        if settings.typo_mode == "Correct with Backspace" && self.pending_wrong {
            self.tip = Tip::PressBackspace;
            return CharacterOutcome::Blocked;
        }
        let Some(&expected) = self.text.get(self.index) else {
            return CharacterOutcome::Ignored;
        };
        let now = clock.now();
        let delay = now - self.last_key;
        self.last_key = now;
        let timed_out = delay > settings.timeout_seconds;
        let correct = actual == expected;
        self.typed_keys += 1;
        if correct {
            self.correct_keys += 1;
        }
        self.strokes.push(Stroke {
            expected: expected.to_string(),
            actual: actual.to_string(),
            timestamp: now,
            delay,
            correct,
            timed_out,
            index: self.index,
            system_key: false,
        });
        if timed_out {
            self.timeouts += 1;
        }
        if correct {
            self.states.insert(
                self.index,
                if timed_out { CharState::Timeout } else { CharState::Correct },
            );
            self.index += 1;
            self.pending_wrong = false;
        } else {
            self.errors += 1;
            self.fixed_indices.insert(self.index);
            self.states.insert(
                self.index,
                if timed_out { CharState::ErrorTimeout } else { CharState::Error },
            );
            match settings.typo_mode.as_str() {
                "Continue" => self.index += 1,
                "Correct with Backspace" => self.pending_wrong = true,
                _ => {}
            }
        }
        if self.index >= self.text.len() {
            CharacterOutcome::Finished
        } else {
            CharacterOutcome::Accepted { correct }
        }
    }

    pub fn finish(
        &self,
        settings: &Settings,
        clock: &dyn Clock,
        course_file: &str,
        custom_course: bool,
    ) -> Attempt {
        let now = clock.now();
        let duration = self.elapsed(now).max(0.01);
        let normal: Vec<&Stroke> = self.strokes.iter().filter(|s| !s.system_key).collect();
        let correct = normal.iter().filter(|s| s.correct).count();
        let accuracy = if normal.is_empty() {
            100.0
        } else {
            correct as f64 / normal.len() as f64 * 100.0
        };
        let wpm = correct as f64 / 5.0 / (duration / 60.0);
        let slowdown = if normal.is_empty() {
            0.0
        } else {
            self.timeouts as f64 / normal.len() as f64 * 100.0
        };
        let text_string: String = self.text.iter().collect();
        let fixed_words = self
            .fixed_indices
            .iter()
            .filter_map(|&i| word_at(&text_string, i))
            .collect::<HashSet<String>>()
            .len();
        let mut states: HashMap<String, String> = self
            .states
            .iter()
            .map(|(k, v)| (k.to_string(), v.as_str().to_string()))
            .collect();
        for index in &self.fixed_indices {
            let entry = states
                .get(&index.to_string())
                .cloned()
                .unwrap_or_else(|| "correct".to_string());
            states.insert(
                index.to_string(),
                if entry == "timeout" {
                    "error_timeout".to_string()
                } else {
                    "error".to_string()
                },
            );
        }
        Attempt {
            lesson: self.lesson.clone(),
            started_at: self.started_wall.clone(),
            course_file: course_file.to_string(),
            custom_course,
            finished_at: clock.wall_iso(),
            duration,
            wpm,
            accuracy,
            slowdown,
            done: if self.text.is_empty() {
                0.0
            } else {
                self.index as f64 / self.text.len() as f64 * 100.0
            },
            words: text_string
                .chars()
                .take(self.index)
                .collect::<String>()
                .split_whitespace()
                .count(),
            fixed_words,
            characters: self.index,
            fixed_characters: self.fixed_indices.len(),
            errors: self.errors,
            timeouts: self.timeouts,
            backspaces: self.backspaces,
            passed: wpm >= settings.goal_wpm && accuracy >= settings.goal_accuracy,
            text: text_string,
            states,
            strokes: self.strokes.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LiveMetrics {
    pub elapsed: f64,
    pub wpm: f64,
    pub accuracy: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterOutcome {
    Accepted { correct: bool },
    Blocked,
    Ignored,
    Finished,
}

/// Word containing `index`, or `None` on whitespace / out of bounds.
/// Mirrors `TypingApp._word_at()`.
pub fn word_at(text: &str, index: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let &c = chars.get(index)?;
    if c.is_whitespace() {
        return None;
    }
    let mut start = index;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = index;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

/// Overall score 0..100, mirroring `ResultsPage._score()`.
pub fn overall_score(wpm: f64, accuracy: f64, slowdown: f64, goal_wpm: f64) -> f64 {
    let speed = (wpm / goal_wpm.max(1.0) * 100.0).min(100.0);
    ((accuracy + speed + (100.0 - slowdown)) / 3.0).clamp(0.0, 100.0)
}

/// Display scale, mirroring `ui_scaling.display_scale()`.
/// egui handles real scaling; this is kept for behaviour parity + tests.
pub fn display_scale(tk_dpi: f64, desktop_dpi: Option<f64>, platform: &str) -> f64 {
    if !platform.starts_with("linux") {
        return 1.0;
    }
    let dpi = desktop_dpi.unwrap_or(tk_dpi);
    if !dpi.is_finite() || dpi <= 0.0 {
        return 1.0;
    }
    let scale = (dpi / 96.0).clamp(0.75, 4.0);
    let quarter = (scale * 4.0).round() / 4.0;
    if (scale - quarter).abs() < 0.04 {
        quarter
    } else {
        (scale * 100.0).round() / 100.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardDims {
    pub key_width: i64,
    pub key_height: i64,
    pub gap: i64,
    pub font_size: i64,
    pub corner: i64,
    pub row_gap: i64,
    pub space_width: i64,
    pub space_height: i64,
}

/// Exact port of `TypingApp._keyboard_dimensions()`.
pub fn keyboard_dimensions(width: f64, height: f64, row_lengths: &[usize]) -> KeyboardDims {
    let longest = row_lengths.iter().copied().max().unwrap_or(1).max(1) as f64;
    let rows = row_lengths.len().max(1) as f64;
    let target_width = (width * 0.82).max(1.0);
    let target_height = (height * 0.82).max(1.0);
    let gap = (width * 0.003)
        .min(target_width / (longest * 8.0))
        .min(7.0) as i64;
    let gap = gap.max(0);
    let row_gap = (height * 0.012)
        .min(target_height / ((rows + 1.0) * 8.0))
        .min(7.0) as i64;
    let row_gap = row_gap.max(0);
    let key_width = ((target_width / longest) as i64 - gap * 2).max(1);
    let key_height =
        (((target_height - row_gap as f64 * 2.0 * (rows + 1.0)) / (rows + 0.82)) as i64).max(1);
    let space_height = ((key_height as f64 * 0.82) as i64).max(1);
    let longest_min = longest.min(6.8);
    let space_width =
        (target_width as i64).min((key_width as f64 * longest_min) as i64).max(1);
    let font_size = (36).min((key_height as f64 * 0.60) as i64).min((key_width as f64 * 0.64) as i64).max(1);
    let corner = (16).min((key_height as f64 * 0.22) as i64).min(key_width / 2).max(0);
    KeyboardDims { key_width, key_height, gap, font_size, corner, row_gap, space_width, space_height }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StepClock(std::cell::Cell<f64>);
    impl Clock for StepClock {
        fn now(&self) -> f64 {
            self.0.get()
        }
        fn wall_iso(&self) -> String {
            "2026-01-01T00:00:00".to_string()
        }
    }

    #[test]
    fn correct_flow_finishes() {
        let mut settings = Settings::default();
        settings.timeout_seconds = 100.0;
        let clock = StepClock(std::cell::Cell::new(0.0));
        let mut s = Session::new("01", "ab");
        s.start(&clock);
        clock.0.set(0.5);
        assert!(matches!(
            s.handle_character(&settings, &clock, 'a'),
            CharacterOutcome::Accepted { correct: true }
        ));
        clock.0.set(1.0);
        assert_eq!(
            s.handle_character(&settings, &clock, 'b'),
            CharacterOutcome::Finished
        );
        let attempt = s.finish(&settings, &clock, "01.txt", false);
        assert_eq!(attempt.accuracy, 100.0);
        assert_eq!(attempt.errors, 0);
    }

    #[test]
    fn typo_modes() {
        for (mode, should_advance) in [
            ("Type the right character", false),
            ("Correct with Backspace", false),
            ("Continue", true),
        ] {
            let mut settings = Settings::default();
            settings.typo_mode = mode.to_string();
            settings.timeout_seconds = 100.0;
            let clock = StepClock(std::cell::Cell::new(0.0));
            let mut s = Session::new("01", "ab");
            s.start(&clock);
            clock.0.set(0.2);
            s.handle_character(&settings, &clock, 'x');
            assert_eq!(s.index, if should_advance { 1 } else { 0 }, "mode {mode}");
        }
    }

    #[test]
    fn backspace_repairs_pending() {
        let mut settings = Settings::default();
        settings.typo_mode = "Correct with Backspace".to_string();
        settings.timeout_seconds = 100.0;
        let clock = StepClock(std::cell::Cell::new(0.0));
        let mut s = Session::new("01", "ab");
        s.start(&clock);
        clock.0.set(0.2);
        s.handle_character(&settings, &clock, 'x');
        assert!(s.pending_wrong);
        clock.0.set(0.3);
        s.handle_backspace(&settings, &clock);
        assert!(!s.pending_wrong);
        assert_eq!(s.states.get(&0), None);
    }

    #[test]
    fn word_at_bounds() {
        assert_eq!(word_at("hello world", 1).as_deref(), Some("hello"));
        assert_eq!(word_at("hello world", 5), None);
        assert_eq!(word_at("", 0), None);
    }

    #[test]
    fn display_scale_parity() {
        assert_eq!(display_scale(96.0, Some(96.0), "linux"), 1.0);
        assert_eq!(display_scale(96.0, Some(120.0), "linux"), 1.25);
        assert_eq!(display_scale(95.90, None, "linux"), 1.0);
        assert_eq!(display_scale(96.0, Some(192.0), "win32"), 1.0);
        assert_eq!(display_scale(f64::NAN, None, "linux"), 1.0);
    }

    #[test]
    fn keyboard_fits_viewports() {
        let layouts: Vec<Vec<usize>> =
            vec![vec![13, 12, 12, 11], vec![14, 13, 13, 12, 10], vec![20, 20, 20, 20, 20, 20]];
        for widths in &layouts {
            for (w, h) in [(320.0, 80.0), (1050.0, 220.0), (1920.0, 430.0)] {
                let d = keyboard_dimensions(w, h, &widths);
                let longest = *widths.iter().max().unwrap() as i64;
                let actual_w =
                    (longest * (d.key_width + 2 * d.gap)).max(d.space_width);
                let actual_h = widths.len() as i64 * (d.key_height + 2 * d.row_gap)
                    + d.space_height
                    + 2 * d.row_gap;
                assert!(actual_w as f64 <= w, "w={w} got {actual_w}");
                assert!(actual_h as f64 <= h, "h={h} got {actual_h}");
            }
        }
    }
}
