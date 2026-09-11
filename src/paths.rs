//! Writable runtime paths, mirroring `runtime_root()` in `main.py`.
//!
//! * Dev / `cargo run`: the source checkout itself.
//! * Windows installed (beside-EXE `install-info.json`): `%LOCALAPPDATA%/BIT Typing`.
//! * Windows portable: directory beside the EXE.
//! * Linux/macOS installed (`/usr/bin` not writable): `$XDG_DATA_HOME/BIT Typing`
//!   or `~/.local/share/BIT Typing`.

use std::path::{Path, PathBuf};

use rust_embed::RustEmbed;

use crate::{EmbeddedAssets, EmbeddedCourses, EmbeddedKeyboards, EmbeddedLangs, EmbeddedSounds};

pub const APP_NAME: &str = "BIT Typing";
pub const APP_BIN_NAME: &str = "bit-typing";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_WEBSITE: &str = "https://bitiskola.github.io/bit-typing";
pub const UI_REVISION: &str = "rust-egui-1";

/// Lowercase installer id used for `.desktop`, registry, shortcuts.
pub const APP_ID: &str = "bit-typing";

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Writable root for courses / settings / history.
pub fn runtime_root() -> PathBuf {
    // `cargo run` / `cargo test`: keep the checkout writable layout.
    if std::env::var("CARGO_MANIFEST_DIR").is_ok() {
        // Still honour an explicit override for tests.
        if let Ok(dir) = std::env::var("BIT_TYPING_DATA_DIR") {
            return PathBuf::from(dir);
        }
    }
    if let Ok(dir) = std::env::var("BIT_TYPING_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        if let Some(dir) = exe_dir() {
            if dir.join("install-info.json").is_file() {
                let base = std::env::var("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| home_dir().join("AppData").join("Local"));
                return base.join(APP_NAME);
            }
            return dir;
        }
        return home_dir().join("AppData").join("Local").join(APP_NAME);
    }
    #[cfg(not(windows))]
    {
        // Source checkout heuristic: running from `target/` next to Cargo.toml.
        if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
            let _ = manifest;
        }
        // If the executable lives next to `courses/` + `lang/` (portable
        // tarball / dev build), use the source tree directly when it looks
        // like a checkout, otherwise follow XDG like the Python build.
        if let Some(dir) = exe_dir() {
            // `target/debug/bit-typing` -> project root two levels up.
            for ancestor in dir.ancestors().take(4) {
                if ancestor.join("courses").is_dir()
                    && ancestor.join("lang").is_dir()
                    && ancestor.join("Cargo.toml").is_file()
                {
                    return ancestor.to_path_buf();
                }
            }
        }
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".local").join("share"));
        base.join(APP_NAME)
    }
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn courses_dir(root: &Path) -> PathBuf {
    root.join("courses")
}
pub fn langs_dir(root: &Path) -> PathBuf {
    root.join("lang")
}
pub fn keyboards_dir(root: &Path) -> PathBuf {
    root.join("keyboards")
}
pub fn data_dir(root: &Path) -> PathBuf {
    root.join("data")
}
pub fn sounds_dir(root: &Path) -> PathBuf {
    root.join("data").join("sounds")
}
pub fn history_file(root: &Path) -> PathBuf {
    data_dir(root).join("history.json")
}
pub fn settings_file(root: &Path) -> PathBuf {
    data_dir(root).join("settings.json")
}
pub fn custom_courses_file(root: &Path) -> PathBuf {
    data_dir(root).join("custom_courses.json")
}

/// Source-tree fallback for seeds when not running from a checkout
/// (installed Linux package, macOS .app, Windows install).
pub fn bundle_source_dir() -> PathBuf {
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        return PathBuf::from(manifest);
    }
    if let Some(dir) = exe_dir() {
        // macOS: `BIT Typing.app/Contents/MacOS/bit-typing` -> Resources.
        if cfg!(target_os = "macos") {
            let resources = dir.join("../Resources");
            if resources.is_dir() {
                return resources;
            }
        }
        return dir;
    }
    PathBuf::from(".")
}

fn copy_embedded<E: RustEmbed>(dest: &Path) {
    for name in E::iter() {
        let target = dest.join(name.as_ref());
        if target.exists() {
            continue;
        }
        if let Some(file) = E::get(name.as_ref()) {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&target, file.data.as_ref()).is_err() {
                continue;
            }
        }
    }
}

fn copy_dir_seeds(src: &Path, dest: &Path, ext: &str) {
    if !src.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let target = dest.join(path.file_name().unwrap());
        if target.exists() {
            continue;
        }
        let _ = std::fs::copy(&path, &target);
    }
}

/// Create writable dirs and seed bundled defaults without overwriting user data.
pub fn ensure_dirs(root: &Path) {
    for dir in [
        courses_dir(root),
        langs_dir(root),
        keyboards_dir(root),
        data_dir(root),
        sounds_dir(root),
    ] {
        let _ = std::fs::create_dir_all(&dir);
    }

    // Never overwrite user-edited lessons / translations / layouts.
    copy_embedded::<EmbeddedCourses>(&courses_dir(root));
    copy_embedded::<EmbeddedLangs>(&langs_dir(root));
    copy_embedded::<EmbeddedKeyboards>(&keyboards_dir(root));
    copy_embedded::<EmbeddedSounds>(&sounds_dir(root));
    copy_embedded::<EmbeddedAssets>(&root.join("assets"));

    // App updates may add new translation keys: merge missing strings into
    // existing lang files so the UI never falls back to key names, while
    // keeping every user edit and custom file intact.
    merge_missing_lang_strings(&langs_dir(root));

    // Filesystem fallback (portable tarballs / system packages where the
    // embedded copy may lag): copy missing seeds from the bundle dir.
    let bundle = bundle_source_dir();
    if bundle != *root {
        copy_dir_seeds(&bundle.join("courses"), &courses_dir(root), "txt");
        copy_dir_seeds(&bundle.join("lang"), &langs_dir(root), "json");
        copy_dir_seeds(&bundle.join("keyboards"), &keyboards_dir(root), "json");
        copy_dir_seeds(&bundle.join("data").join("sounds"), &sounds_dir(root), "wav");
    }
}

/// Add translation keys shipped by newer app versions to already-seeded
/// lang files. Runtime values always win; only missing keys are filled in.
/// Custom user files (absent from the bundle) are left untouched.
fn merge_missing_lang_strings(dir: &Path) {
    for name in EmbeddedLangs::iter() {
        // `iter()` yields prefixed names ("lang/HU-hu.json"); the lang dir
        // itself is flat, so join by file name.
        let file_name = Path::new(name.as_ref())
            .file_name()
            .map(|n| n.to_owned())
            .unwrap_or_default();
        let target = dir.join(file_name);
        let Ok(current) = std::fs::read_to_string(&target) else {
            continue; // missing files are handled by `copy_embedded`
        };
        let Ok(mut cur) = serde_json::from_str::<serde_json::Value>(&current) else {
            continue; // never clobber an unreadable user file
        };
        let Some(embedded) = EmbeddedLangs::get(name.as_ref()) else {
            continue;
        };
        let Ok(emb) = serde_json::from_slice::<serde_json::Value>(&embedded.data) else {
            continue;
        };
        if merge_missing_value(&mut cur, &emb) {
            let _ = write_json(&target, &cur);
        }
    }
}

/// Insert keys present in `emb` but absent in `cur`, recursively.
/// Returns true when anything was added.
fn merge_missing_value(cur: &mut serde_json::Value, emb: &serde_json::Value) -> bool {
    match (cur, emb) {
        (serde_json::Value::Object(c), serde_json::Value::Object(e)) => {
            let mut changed = false;
            for (k, v) in e {
                match c.get_mut(k) {
                    Some(existing) => changed |= merge_missing_value(existing, v),
                    None => {
                        c.insert(k.clone(), v.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        _ => false, // runtime scalars/arrays always win
    }
}

/// Atomic JSON write (`.tmp` + rename), mirroring `write_json()` in Python.
pub fn write_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value).unwrap())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn read_json(path: &Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_root_respects_override() {
        std::env::set_var("BIT_TYPING_DATA_DIR", "/tmp/bit-typing-test-root");
        assert_eq!(runtime_root(), PathBuf::from("/tmp/bit-typing-test-root"));
        std::env::remove_var("BIT_TYPING_DATA_DIR");
    }

    #[test]
    fn lang_merge_adds_missing_keys_keeps_user_edits() {
        let dir = tempfile::tempdir().unwrap();
        let langs = dir.path().join("lang");
        std::fs::create_dir_all(&langs).unwrap();
        // Simulate a stale install: HU pack predating finger_*/cpm_* keys,
        // with one user-customized string, plus an unrelated custom file.
        std::fs::write(
            langs.join("HU-hu.json"),
            r#"{"name":"Magyar","strings":{"current_lesson":"Egyéni","wpm_unit":"WPM"}}"#,
        )
        .unwrap();
        std::fs::write(langs.join("ZZ-custom.json"), r#"{"x":1}"#).unwrap();
        merge_missing_lang_strings(&langs);
        let merged: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(langs.join("HU-hu.json")).unwrap(),
        )
        .unwrap();
        let strings = merged.get("strings").unwrap();
        // Missing keys filled from the bundle…
        assert!(strings.get("finger_pinky").is_some());
        assert!(strings.get("cpm_unit").is_some());
        // …while user edits win.
        assert_eq!(strings.get("current_lesson").unwrap(), "Egyéni");
        assert_eq!(strings.get("wpm_unit").unwrap(), "WPM");
        // Custom files untouched.
        assert_eq!(
            std::fs::read_to_string(langs.join("ZZ-custom.json")).unwrap(),
            r#"{"x":1}"#
        );
    }
}
