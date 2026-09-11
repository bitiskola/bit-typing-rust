//! Virtual keyboard layouts: `keyboards/<code>.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardLayout {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
    #[serde(default)]
    pub space_label: String,
}

impl KeyboardLayout {
    pub fn row_lengths(&self) -> Vec<usize> {
        self.rows.iter().map(|r| r.len()).collect()
    }
}

pub fn load_layouts(dir: &Path) -> std::collections::BTreeMap<String, KeyboardLayout> {
    let mut map = std::collections::BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let code = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(layout) = serde_json::from_str::<KeyboardLayout>(&text) {
                    map.insert(code, layout);
                }
            }
        }
    }
    map
}
