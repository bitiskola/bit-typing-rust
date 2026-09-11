//! Settings schema, mirroring `DEFAULT_SETTINGS` in `main.py`.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub setup_complete: bool,
    #[serde(default = "default_typo_mode")]
    pub typo_mode: String,
    #[serde(default = "default_true")]
    pub backspace: bool,
    #[serde(default = "default_goal_wpm")]
    pub goal_wpm: f64,
    #[serde(default = "default_goal_accuracy")]
    pub goal_accuracy: f64,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: f64,
    #[serde(default)]
    pub timed_minutes: f64,
    #[serde(default)]
    pub metronome: bool,
    #[serde(default)]
    pub sound: bool,
    #[serde(default = "default_true")]
    pub auto_start: bool,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_keyboard")]
    pub keyboard_layout: String,
    /// Speed unit for statistics ("wpm" | "cpm"). Mirrors Swift `SpeedUnit`
    /// persisted in UserDefaults; shared so every speed readout is consistent.
    #[serde(default = "default_speed_unit")]
    pub speed_unit: String,
}

fn default_typo_mode() -> String {
    "Type the right character".to_string()
}
fn default_true() -> bool {
    true
}
fn default_goal_wpm() -> f64 {
    30.0
}
fn default_goal_accuracy() -> f64 {
    95.0
}
fn default_timeout() -> f64 {
    1.6
}
fn default_language() -> String {
    "EN-us".to_string()
}
fn default_keyboard() -> String {
    "US-qwerty".to_string()
}
fn default_speed_unit() -> String {
    "wpm".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({
            "setup_complete": false,
            "typo_mode": default_typo_mode(),
            "backspace": true,
            "goal_wpm": 30.0,
            "goal_accuracy": 95.0,
            "timeout_seconds": 1.6,
            "timed_minutes": 0.0,
            "metronome": false,
            "sound": false,
            "auto_start": true,
            "language": default_language(),
            "keyboard_layout": default_keyboard(),
            "speed_unit": default_speed_unit(),
        }))
        .expect("default settings")
    }
}

pub const TYPO_MODES: &[&str] = &[
    "Type the right character",
    "Correct with Backspace",
    "Continue",
];

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut base = Settings::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return base;
        };
        let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else {
            return base;
        };
        // Merge over defaults so old / partial files keep working.
        if let Ok(merged) = serde_json::from_value::<Settings>(merge(
            serde_json::to_value(&base).unwrap(),
            value,
        )) {
            base = merged;
        }
        base.normalise();
        base
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::paths::write_json(path, &serde_json::to_value(self).unwrap())
    }

    /// Clamp numeric fields; mirrors the Options validation in Python.
    pub fn normalise(&mut self) {
        if !TYPO_MODES.contains(&self.typo_mode.as_str()) {
            self.typo_mode = default_typo_mode();
        }
        if self.speed_unit != "cpm" && self.speed_unit != "wpm" {
            self.speed_unit = default_speed_unit();
        }
        self.goal_wpm = self.goal_wpm.max(1.0);
        self.goal_accuracy = self.goal_accuracy.clamp(0.0, 100.0);
        self.timeout_seconds = self.timeout_seconds.max(0.1);
        self.timed_minutes = self.timed_minutes.max(0.0);
    }
}

fn merge(mut base: serde_json::Value, over: serde_json::Value) -> serde_json::Value {
    if let (Some(b), Some(o)) = (base.as_object_mut(), over.as_object()) {
        for (k, v) in o {
            b.insert(k.clone(), v.clone());
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_python() {
        let s = Settings::default();
        assert!(!s.setup_complete);
        assert_eq!(s.typo_mode, "Type the right character");
        assert!(s.backspace);
        assert_eq!(s.goal_wpm, 30.0);
        assert_eq!(s.goal_accuracy, 95.0);
        assert_eq!(s.timeout_seconds, 1.6);
        assert_eq!(s.timed_minutes, 0.0);
        assert_eq!(s.language, "EN-us");
        assert_eq!(s.keyboard_layout, "US-qwerty");
    }

    #[test]
    fn partial_file_merges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"language":"HU-hu","goal_wpm":40}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.language, "HU-hu");
        assert_eq!(s.goal_wpm, 40.0);
        assert_eq!(s.keyboard_layout, "US-qwerty");
    }
}
