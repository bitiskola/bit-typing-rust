//! Internationalisation: `lang/<code>.json` files.
//!
//! Format: `{ "name": "...", "default_keyboard": "...", "strings": { key: text } }`.
//! Lookup falls back to English, then to a Title-Case version of the key.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LangFile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub default_keyboard: String,
    #[serde(default)]
    pub strings: HashMap<String, String>,
}

pub struct I18n {
    codes: Vec<String>,
    files: HashMap<String, LangFile>,
    english: HashMap<String, String>,
}

impl I18n {
    pub fn load(dir: &Path) -> Self {
        let mut files = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let code = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(lang) = serde_json::from_str::<LangFile>(&text) {
                        files.insert(code, lang);
                    }
                }
            }
        }
        let mut codes: Vec<String> = files.keys().cloned().collect();
        codes.sort();
        let english = files.get("EN-us").map(|l| l.strings.clone()).unwrap_or_default();
        Self { codes, files, english }
    }

    pub fn codes(&self) -> &[String] {
        &self.codes
    }

    pub fn display_name(&self, code: &str) -> String {
        match self.files.get(code) {
            Some(lang) if !lang.name.is_empty() => {
                // Disambiguate duplicate friendly names like Python does.
                let collisions = self
                    .files
                    .values()
                    .filter(|l| l.name == lang.name)
                    .count();
                if collisions > 1 {
                    format!("{} ({})", lang.name, code)
                } else {
                    lang.name.clone()
                }
            }
            _ => code.to_string(),
        }
    }

    pub fn default_keyboard(&self, code: &str) -> Option<String> {
        self.files
            .get(code)
            .map(|l| l.default_keyboard.clone())
            .filter(|k| !k.is_empty())
    }

    /// Translate `key` for `language`, with EN + TitleCase fallback.
    pub fn t(&self, language: &str, key: &str) -> String {
        if let Some(lang) = self.files.get(language) {
            if let Some(s) = lang.strings.get(key) {
                return s.clone();
            }
        }
        if let Some(s) = self.english.get(key) {
            return s.clone();
        }
        title_case(key)
    }

    /// `history_summary` template with `{count}`, `{best:.1f}`, `{unit}`, `{accuracy:.1f}`.
    pub fn history_summary(
        &self,
        language: &str,
        count: usize,
        best: f64,
        unit: &str,
        accuracy: f64,
    ) -> String {
        let template = self.t(language, "history_summary");
        template
            .replace("{count}", &count.to_string())
            .replace("{best:.1f}", &format!("{best:.1}"))
            .replace("{unit}", unit)
            .replace("{accuracy:.1f}", &format!("{accuracy:.1}"))
    }
}

fn title_case(key: &str) -> String {
    key.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_chain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("EN-us.json"),
            r#"{"name":"English (US)","default_keyboard":"US-qwerty","strings":{"hello":"Hello"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("HU-hu.json"),
            r#"{"name":"Magyar","default_keyboard":"HU-qwertz","strings":{}}"#,
        )
        .unwrap();
        let i18n = I18n::load(dir.path());
        assert_eq!(i18n.t("HU-hu", "hello"), "Hello");
        assert_eq!(i18n.t("HU-hu", "some_key"), "Some Key");
    }
}
