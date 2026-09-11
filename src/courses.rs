//! Course file reading with legacy-encoding support.
//!
//! Faithful port of `read_course_text()` in `main.py`:
//! BOM sniffing, BOM-less UTF-16/32 NUL heuristics, then
//! UTF-8 / CP1250 / CP1252 / Latin-1, with universal-newline normalisation.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Built-in lessons are never treated as "custom" (no *Next lesson* skip).
pub const BUILTIN_COURSE_FILES: &[&str] =
    &["01-Home-row.txt", "02-Common-words.txt", "03-Full-keyboard.txt"];

pub fn read_course_text(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.is_empty() {
        return Ok(String::new());
    }
    let text = decode_bytes(&data).ok_or_else(|| "undecodable lesson file".to_string())?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn decode_bytes(data: &[u8]) -> Option<String> {
    if data.starts_with(b"\xff\xfe\x00\x00") || data.starts_with(b"\x00\x00\xfe\xff") {
        return decode_utf32_with_bom(data);
    }
    if data.starts_with(b"\xff\xfe") || data.starts_with(b"\xfe\xff") {
        return decode_utf16_with_bom(data);
    }
    if data.starts_with(b"\xef\xbb\xbf") {
        return String::from_utf8(data[3..].to_vec()).ok();
    }
    let mut candidates: Vec<&'static encoding_rs::Encoding> = Vec::new();
    let mut utf32_le_bomless = false;
    let mut utf32_be_bomless = false;

    // BOM-less UTF-32 heuristic: repeated NUL lanes.
    let sample_len = data.len().min(8192);
    let sample = &data[..sample_len];
    if sample.len() >= 8 {
        let mut ratios = [0f64; 4];
        for lane in 0..4 {
            let lane_bytes: Vec<u8> = sample.iter().skip(lane).step_by(4).copied().collect();
            if !lane_bytes.is_empty() {
                ratios[lane] =
                    lane_bytes.iter().filter(|&&b| b == 0).count() as f64 / lane_bytes.len() as f64;
            }
        }
        if ratios[1..].iter().all(|&r| r > 0.6) && ratios[0] < 0.4 {
            utf32_le_bomless = true;
        } else if ratios[..3].iter().all(|&r| r > 0.6) && ratios[3] < 0.4 {
            utf32_be_bomless = true;
        }
    }
    if sample.len() >= 4 {
        let even: Vec<u8> = sample.iter().step_by(2).copied().collect();
        let odd: Vec<u8> = sample.iter().skip(1).step_by(2).copied().collect();
        if !even.is_empty() && !odd.is_empty() {
            let even_nuls = even.iter().filter(|&&b| b == 0).count() as f64 / even.len() as f64;
            let odd_nuls = odd.iter().filter(|&&b| b == 0).count() as f64 / odd.len() as f64;
            if odd_nuls > 0.6 && even_nuls < 0.4 {
                candidates.push(encoding_rs::UTF_16LE);
            } else if even_nuls > 0.6 && odd_nuls < 0.4 {
                candidates.push(encoding_rs::UTF_16BE);
            }
        }
    }

    candidates.push(encoding_rs::UTF_8);
    candidates.push(encoding_rs::WINDOWS_1250);
    candidates.push(encoding_rs::WINDOWS_1252);

    // BOM-less UTF-32 attempted first: encoding_rs has no UTF-32 codec, so
    // decode manually (strict: reject lone surrogates / out-of-range).
    if utf32_le_bomless {
        if let Some(text) = decode_utf32_body(data, true) {
            return Some(text);
        }
    }
    if utf32_be_bomless {
        if let Some(text) = decode_utf32_body(data, false) {
            return Some(text);
        }
    }

    for enc in candidates {
        let (text, _, had_errors) = enc.decode(data);
        if !had_errors {
            return Some(text.into_owned());
        }
    }
    // Latin-1 never fails.
    Some(data.iter().map(|&b| b as char).collect())
}

fn decode_utf16_with_bom(data: &[u8]) -> Option<String> {
    let le = data.starts_with(b"\xff\xfe");
    let enc = if le {
        encoding_rs::UTF_16LE
    } else {
        encoding_rs::UTF_16BE
    };
    let (text, _, _) = enc.decode(&data[2..]);
    Some(text.into_owned())
}

fn decode_utf32_with_bom(data: &[u8]) -> Option<String> {
    let le = data.starts_with(b"\xff\xfe\x00\x00");
    decode_utf32_body(&data[4..], le)
}

fn decode_utf32_body(data: &[u8], le: bool) -> Option<String> {
    if data.len() % 4 != 0 {
        return None;
    }
    let mut out = String::with_capacity(data.len() / 4);
    for chunk in data.chunks_exact(4) {
        let value = if le {
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        } else {
            u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        };
        out.push(char::from_u32(value)?);
    }
    Some(out)
}

/// Sorted lesson files (`casefold` order like Python).
pub fn list_courses(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("txt"))
        .collect();
    paths.sort_by_key(|p| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    paths
}

pub fn load_custom_courses(path: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

pub fn save_custom_courses(path: &Path, set: &BTreeSet<String>) -> std::io::Result<()> {
    let mut list: Vec<&String> = set.iter().collect();
    list.sort();
    crate::paths::write_json(path, &serde_json::to_value(list).unwrap())
}

/// Keep only filename-safe characters, mirroring the editor's sanitiser.
pub fn sanitise_lesson_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Next numbered non-custom lesson, mirroring `next_lesson_path()`.
pub fn next_lesson(
    course_paths: &[PathBuf],
    custom: &BTreeSet<String>,
    course_file: &str,
    lesson_stem: &str,
    is_custom_attempt: bool,
) -> Option<PathBuf> {
    let current = course_paths
        .iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(course_file))
        .or_else(|| {
            course_paths
                .iter()
                .find(|p| p.file_stem().and_then(|n| n.to_str()) == Some(lesson_stem))
        })?;
    let current_name = current.file_name()?.to_string_lossy().to_string();
    if custom.contains(&current_name) || is_custom_attempt {
        return None;
    }
    let current_number = leading_number(&current.file_stem()?.to_string_lossy())?;
    let mut best: Option<(u64, &PathBuf)> = None;
    for path in course_paths {
        if path == current {
            continue;
        }
        let name = path.file_name()?.to_string_lossy().to_string();
        if custom.contains(&name) {
            continue;
        }
        let stem = path.file_stem()?.to_string_lossy().to_string();
        let Some(n) = leading_number(&stem) else {
            continue;
        };
        if n > current_number {
            let replace = match &best {
                None => true,
                Some((bn, bp)) => {
                    n < *bn
                        || (n == *bn
                            && stem.to_lowercase()
                                < bp.file_stem()?.to_string_lossy().to_lowercase())
                }
            };
            if replace {
                best = Some((n, path));
            }
        }
    }
    best.map(|(_, p)| p.clone())
}

fn leading_number(stem: &str) -> Option<u64> {
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// SHA-256 inventory over the three default groups (for `verify` tooling).
pub fn inventory(project: &Path) -> BTreeMap<String, String> {
    use sha2::{Digest, Sha256};
    let mut files = BTreeMap::new();
    for (folder, ext) in [("courses", "txt"), ("lang", "json"), ("keyboards", "json")] {
        let dir = project.join(folder);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(bytes) = std::fs::read(&path) {
                let mut h = Sha256::new();
                h.update(&bytes);
                let digest: String =
                    h.finalize().iter().map(|b| format!("{b:02x}")).collect();
                let rel = path
                    .strip_prefix(project)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(rel, digest);
            }
        }
    }
    files
}

use std::collections::BTreeMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_and_newlines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "hello\r\nworld\r!").unwrap();
        assert_eq!(read_course_text(&p).unwrap(), "hello\nworld\n!");
    }

    #[test]
    fn utf16le_bom() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.txt");
        let mut bytes = vec![0xFF, 0xFE];
        for u in "árvíz".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(read_course_text(&p).unwrap(), "árvíz");
    }

    #[test]
    fn sanitise() {
        assert_eq!(sanitise_lesson_name("  My Lesson: 01! "), "My Lesson 01");
    }

    #[test]
    fn next_lesson_picks_minimum_greater() {
        let paths = vec![
            PathBuf::from("courses/01.txt"),
            PathBuf::from("courses/02.txt"),
            PathBuf::from("courses/10.txt"),
        ];
        let custom = BTreeSet::new();
        let next = next_lesson(&paths, &custom, "01.txt", "01", false).unwrap();
        assert_eq!(next, PathBuf::from("courses/02.txt"));
        assert!(next_lesson(&paths, &custom, "10.txt", "10", false).is_none());
    }
}
