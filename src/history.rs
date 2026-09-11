//! Attempt / stroke history, mirroring the `history.json` schema in `main.py`.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stroke {
    pub expected: String,
    pub actual: String,
    pub timestamp: f64,
    pub delay: f64,
    pub correct: bool,
    pub timed_out: bool,
    pub index: usize,
    #[serde(default)]
    pub system_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub lesson: String,
    pub started_at: String,
    pub course_file: String,
    #[serde(default)]
    pub custom_course: bool,
    pub finished_at: String,
    pub duration: f64,
    pub wpm: f64,
    pub accuracy: f64,
    pub slowdown: f64,
    pub done: f64,
    pub words: usize,
    pub fixed_words: usize,
    pub characters: usize,
    pub fixed_characters: usize,
    pub errors: usize,
    pub timeouts: usize,
    pub backspaces: usize,
    pub passed: bool,
    pub text: String,
    pub states: HashMap<String, String>,
    pub strokes: Vec<Stroke>,
}

impl Attempt {
    /// Overall score 0..100, mirroring `ResultsPage._score()`.
    pub fn score(&self, goal_wpm: f64) -> f64 {
        let speed = (self.wpm / goal_wpm.max(1.0) * 100.0).min(100.0);
        ((self.accuracy + speed + (100.0 - self.slowdown)) / 3.0).clamp(0.0, 100.0)
    }

    pub fn stars(&self, goal_wpm: f64) -> usize {
        (self.score(goal_wpm) / 20.0).round().clamp(1.0, 5.0) as usize
    }
}

pub fn load_history(path: &Path) -> Vec<Attempt> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Attempt>>(&t).ok())
        .unwrap_or_default()
}

pub fn save_history(path: &Path, history: &[Attempt]) -> std::io::Result<()> {
    crate::paths::write_json(path, &serde_json::to_value(history).unwrap())
}
