//! Course repositories: git-backed lesson packs filtered by keyboard layout.
//!
//! Layout on disk (`<root>/repos/<slug>/`):
//! ```text
//! root/
//! - details.json
//! - HU-qwertz/
//!   - examplecourse.txt
//! - US-qwerty/
//!   - examplecourse.txt
//! ```
//!
//! `details.json` declares the repo name, creator and which file belongs to
//! which layout:
//! ```json
//! {
//!   "name": "BIT Main Courses",
//!   "creator": "bitiskola",
//!   "courses": {
//!     "HU-qwertz": ["HU-qwertz/examplecourse.txt"],
//!     "US-qwerty": ["US-qwerty/examplecourse.txt"]
//!   }
//! }
//! ```
//! A list form is accepted as well:
//! ```json
//! {
//!   "name": "BIT Main Courses",
//!   "creator": "bitiskola",
//!   "courses": [
//!     {"layout": "HU-qwertz", "path": "HU-qwertz/examplecourse.txt"}
//!   ]
//! }
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Default course pack. Always present, never removable.
pub const DEFAULT_REPO_URL: &str = "https://github.com/bitiskola/bit-typing-main";

/// One course file inside a repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCourse {
    pub layout: String,
    pub path: String,
    #[serde(default)]
    pub title: Option<String>,
}

/// Parsed `details.json`.
#[derive(Debug, Clone)]
pub struct RepoDetails {
    pub name: String,
    pub creator: String,
    pub courses: Vec<RepoCourse>,
}

/// A lesson available to the user (local file or repo file).
#[derive(Debug, Clone)]
pub struct AvailableCourse {
    /// Stable unique id: `local:<stem>` or `repo:<slug>:<layout>:<stem>`.
    pub id: String,
    /// Short name shown in the selector (`examplecourse`).
    pub title: String,
    /// Repo display name (`""` for local files).
    pub repo_name: String,
    /// Keyboard layout this course belongs to (`"*"` = all layouts).
    pub layout: String,
    /// Absolute file to read.
    pub path: PathBuf,
    /// Identifier persisted in history (`file name` for local, `id` for repos).
    pub course_file: String,
    /// True for user-created local lessons (no *Next lesson* skip logic).
    pub custom: bool,
}

impl AvailableCourse {
    /// Selector label. Repo courses carry their pack name so two packs
    /// shipping the same file name stay distinguishable.
    pub fn display(&self) -> String {
        if self.repo_name.is_empty() {
            self.title.clone()
        } else {
            format!("{} ({})", self.title, self.repo_name)
        }
    }
}

// ---------------------------------------------------------------------------
// Repo list persistence (`data/course_repos.json`)
// ---------------------------------------------------------------------------

/// Normalise a URL for default-repo comparison (trailing `/`, `.git`).
fn normalise_url(url: &str) -> String {
    let mut s = url.trim().to_string();
    while s.ends_with('/') || s.ends_with('\\') {
        s.pop();
    }
    if s.to_lowercase().ends_with(".git") {
        s.truncate(s.len() - 4);
    }
    s
}

pub fn is_default_repo(url: &str) -> bool {
    normalise_url(url) == normalise_url(DEFAULT_REPO_URL)
}

/// Filesystem-safe slug for a repo URL (`.../bit-typing-main` -> `bit-typing-main`).
pub fn slug_for_url(url: &str) -> String {
    let clean = normalise_url(url);
    // Split on both separators: URLs use `/`, Windows paths (used as local
    // clone sources, e.g. in tests) use `\`.
    let last = clean.rsplit(['/', '\\']).next().unwrap_or("repo").trim();
    let mut slug: String = last
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    slug = slug.trim_matches(|c| c == '-' || c == '.' || c == '_').to_string();
    if slug.is_empty() {
        slug = "repo".to_string();
    }
    // Cap length so deep paths stay portable.
    if slug.len() > 64 {
        slug.truncate(64);
    }
    slug
}

pub fn repo_dir_for_url(repos_root: &Path, url: &str) -> PathBuf {
    repos_root.join(slug_for_url(url))
}

/// Load repo URLs. Missing file -> `[DEFAULT]`. The default is always
/// inserted at the front when absent. Accepts both `["url", ...]` and
/// `[{"url": "..."}, ...]` for forward compatibility.
pub fn load_repo_urls(path: &Path) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(arr) = v.as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if !s.trim().is_empty() {
                            urls.push(s.trim().to_string());
                        }
                    } else if let Some(s) = item.get("url").and_then(|u| u.as_str()) {
                        if !s.trim().is_empty() {
                            urls.push(s.trim().to_string());
                        }
                    }
                }
            }
        }
    }
    ensure_default_repo(&mut urls);
    // De-duplicate, keeping order.
    let mut seen = BTreeSet::new();
    urls.retain(|u| seen.insert(normalise_url(u)));
    urls
}

pub fn ensure_default_repo(urls: &mut Vec<String>) {
    if !urls.iter().any(|u| is_default_repo(u)) {
        urls.insert(0, DEFAULT_REPO_URL.to_string());
    }
}

pub fn save_repo_urls(path: &Path, urls: &[String]) -> std::io::Result<()> {
    let mut owned: Vec<String> = urls.to_vec();
    ensure_default_repo(&mut owned);
    crate::paths::write_json(path, &serde_json::to_value(&owned).unwrap())
}

// ---------------------------------------------------------------------------
// details.json
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DetailsRaw {
    #[serde(default)]
    name: String,
    #[serde(default)]
    creator: String,
    #[serde(default)]
    courses: CoursesRaw,
    // `layouts` accepted as an alias for `courses`.
    #[serde(default)]
    layouts: CoursesRaw,
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum CoursesRaw {
    #[default]
    None,
    Map(std::collections::BTreeMap<String, Vec<String>>),
    List(Vec<CourseEntryRaw>),
}

#[derive(Debug, Deserialize)]
struct CourseEntryRaw {
    #[serde(default)]
    layout: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn parse_details(text: &str) -> Result<RepoDetails, String> {
    let raw: DetailsRaw = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut courses: Vec<RepoCourse> = Vec::new();
    let push = |layout: String, path: String, title: Option<String>, out: &mut Vec<RepoCourse>| {
        let layout = layout.trim().to_string();
        let path = path.trim().to_string();
        if layout.is_empty() || path.is_empty() {
            return;
        }
        out.push(RepoCourse { layout, path, title });
    };
    match raw.courses {
        CoursesRaw::Map(map) => {
            for (layout, files) in map {
                for f in files {
                    push(layout.clone(), f, None, &mut courses);
                }
            }
        }
        CoursesRaw::List(list) => {
            for e in list {
                let p = if e.path.is_empty() { e.file } else { e.path };
                let title = e.title.or(e.name);
                push(e.layout, p, title, &mut courses);
            }
        }
        CoursesRaw::None => {}
    }
    // Alias key.
    match raw.layouts {
        CoursesRaw::Map(map) => {
            for (layout, files) in map {
                for f in files {
                    if !courses.iter().any(|c| c.layout == layout && c.path == f) {
                        push(layout.clone(), f, None, &mut courses);
                    }
                }
            }
        }
        CoursesRaw::List(list) => {
            for e in list {
                let p = if e.path.is_empty() { e.file } else { e.path };
                let title = e.title.or(e.name);
                if !courses.iter().any(|c| c.layout == e.layout && c.path == p) {
                    push(e.layout, p, title, &mut courses);
                }
            }
        }
        CoursesRaw::None => {}
    }
    Ok(RepoDetails { name: raw.name, creator: raw.creator, courses })
}

pub fn load_details(repo_dir: &Path) -> Result<RepoDetails, String> {
    let path = repo_dir.join("details.json");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    parse_details(&text)
}

/// Fallback when `details.json` is missing/invalid: every `<layout>/*.txt`
/// subfolder is treated as that layout's pack.
fn scan_fallback(repo_dir: &Path, slug: &str) -> RepoDetails {
    let mut courses = Vec::new();
    let Ok(entries) = std::fs::read_dir(repo_dir) else {
        return RepoDetails { name: slug.to_string(), creator: String::new(), courses };
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let layout = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        if layout.starts_with('.') || layout == ".git" {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("txt") {
                let rel = format!("{layout}/{}", p.file_name().unwrap_or_default().to_string_lossy());
                courses.push(RepoCourse { layout: layout.clone(), path: rel, title: None });
            }
        }
    }
    courses.sort_by(|a, b| (&a.layout, &a.path).cmp(&(&b.layout, &b.path)));
    RepoDetails { name: slug.to_string(), creator: String::new(), courses }
}

fn details_with_fallback(repo_dir: &Path, slug: &str) -> RepoDetails {
    match load_details(repo_dir) {
        Ok(d) if !d.courses.is_empty() => d,
        Ok(d) => {
            // Empty course list: still allow the folder scan to contribute.
            let mut scanned = scan_fallback(repo_dir, slug);
            if scanned.courses.is_empty() {
                d
            } else {
                if scanned.name == slug && !d.name.is_empty() {
                    scanned.name = d.name.clone();
                }
                if scanned.creator.is_empty() {
                    scanned.creator = d.creator.clone();
                }
                scanned
            }
        }
        Err(_) => scan_fallback(repo_dir, slug),
    }
}

// ---------------------------------------------------------------------------
// git sync
//
// There is no builtin git: the app shells out to a `git` binary, resolved as
//  1. `git` on PATH (the Linux .deb depends on git; dev machines too),
//  2. the MinGit tree bundled beside the app binary by the NSIS installer
//     (`<exe-dir>/git/cmd/git.exe`), for Windows PCs without git installed.
// ---------------------------------------------------------------------------

/// Resolve the git binary to shell out to: system git first (respects the
/// user's config), bundled MinGit next to the app binary as a fallback.
pub fn git_program() -> Option<PathBuf> {
    let probe = Command::new("git").arg("--version").output();
    if matches!(probe, Ok(o) if o.status.success()) {
        return Some(PathBuf::from("git"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bundled = dir.join("git").join("cmd").join("git.exe");
            if bundled.is_file() {
                return Some(bundled);
            }
        }
    }
    None
}

/// True when `dest` is a git checkout git itself accepts (guards against
/// stale/corrupt `.git` dirs left by interrupted clones: those make every
/// later `pull` fail with "not a git repository" forever).
fn checkout_valid(git: &Path, dest: &Path) -> bool {
    Command::new(git)
        .args(["-C", &dest.to_string_lossy(), "rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn clone_fresh(url: &str, slug: &str, dest: &Path, git: &Path) -> Result<RepoDetails, String> {
    let out = Command::new(git)
        .args(["clone", "--depth", "1", url, &dest.to_string_lossy()])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let _ = std::fs::remove_dir_all(dest);
        return Err(if msg.is_empty() { "git clone failed".to_string() } else { msg });
    }
    Ok(details_with_fallback(dest, slug))
}

/// Recover a checkout whose `.git` exists but git rejects (interrupted
/// clone, foreign files): clone into a temp sibling dir first and swap it
/// in, so usable local files are never destroyed by a failed network op.
fn recover_corrupt(
    repos_root: &Path,
    url: &str,
    slug: &str,
    dest: &Path,
    git: &Path,
) -> Result<RepoDetails, String> {
    let tmp = repos_root.join(format!(".{slug}.reclone"));
    let backup = repos_root.join(format!(".{slug}.backup"));
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&backup);
    let out = Command::new(git)
        .args(["clone", "--depth", "1", url, &tmp.to_string_lossy()])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let _ = std::fs::remove_dir_all(&tmp);
        // Offline: keep serving whatever usable files survive locally.
        let details = details_with_fallback(dest, slug);
        if details.courses.is_empty() {
            return Err(if msg.is_empty() { "git clone failed".to_string() } else { msg });
        }
        return Ok(details);
    }
    // Swap in the fresh clone; restore the old checkout if the swap fails.
    let swapped = std::fs::rename(dest, &backup).is_ok()
        && std::fs::rename(&tmp, dest).is_ok();
    if swapped {
        let _ = std::fs::remove_dir_all(&backup);
        return Ok(details_with_fallback(dest, slug));
    }
    let _ = std::fs::rename(&backup, dest);
    let _ = std::fs::remove_dir_all(&tmp);
    let details = details_with_fallback(dest, slug);
    if details.courses.is_empty() {
        return Err("could not replace corrupt course checkout".to_string());
    }
    Ok(details)
}

/// Clone a missing repo or pull an existing one. Returns the parsed details.
/// A checkout whose `.git` exists but git rejects is recovered via a temp-dir
/// re-clone (never destroying usable local files on network failure).
/// Other pull failures (offline, …) still serve the local copy when usable.
/// Network/git failures with nothing usable locally are returned as `Err`.
pub fn refresh_repo(repos_root: &Path, url: &str) -> Result<RepoDetails, String> {
    let slug = slug_for_url(url);
    let dest = repos_root.join(&slug);
    let _ = std::fs::create_dir_all(repos_root);
    let git = git_program();
    if dest.join(".git").exists() {
        match git {
            Some(ref g) if checkout_valid(g, &dest) => {
                let out = Command::new(g)
                    .args(["-C", &dest.to_string_lossy(), "pull", "--ff-only"])
                    .output()
                    .map_err(|e| e.to_string())?;
                if out.status.success() {
                    return Ok(details_with_fallback(&dest, &slug));
                }
                // Offline / diverged / up-to-date edge: still serve locals.
                let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let details = details_with_fallback(&dest, &slug);
                if details.courses.is_empty() {
                    return Err(if msg.is_empty() { "git pull failed".to_string() } else { msg });
                }
                return Ok(details);
            }
            Some(ref g) => {
                return recover_corrupt(repos_root, url, &slug, &dest, g);
            }
            None => {
                return Ok(details_with_fallback(&dest, &slug));
            }
        }
    }
    if dest.is_dir() {
        // Manually placed (non-git) course pack: serve it as-is.
        return Ok(details_with_fallback(&dest, &slug));
    }
    match git {
        Some(ref g) => clone_fresh(url, &slug, &dest, g),
        None => Err(
            "git is not available (not on PATH and no bundled git found next to the app)"
                .to_string(),
        ),
    }
}

/// Refresh every configured repo. Never fails as a whole: per-repo results
/// are returned so the UI can report which packs are stale.
pub fn refresh_all(root: &Path, urls: &[String]) -> Vec<(String, Result<RepoDetails, String>)> {
    let repos_root = crate::paths::repos_dir(root);
    urls.iter()
        .map(|u| (u.clone(), refresh_repo(&repos_root, u)))
        .collect()
}

// ---------------------------------------------------------------------------
// Available-course collection
// ---------------------------------------------------------------------------

/// All lessons for `layout_filter` (`settings.keyboard_layout`):
/// * every `courses/*.txt` local file (all layouts, editor + legacy);
/// * every repo course whose `layout` matches (exact, case-insensitive).
/// Sorted case-insensitively by title, then repo name.
pub fn collect_available_courses(
    root: &Path,
    urls: &[String],
    layout_filter: &str,
    custom: &BTreeSet<String>,
) -> Vec<AvailableCourse> {
    let mut out: Vec<AvailableCourse> = Vec::new();
    // Local flat lessons (backward compatible, visible for every layout).
    for path in crate::courses::list_courses(&crate::paths::courses_dir(root)) {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        if stem.is_empty() {
            continue;
        }
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        out.push(AvailableCourse {
            id: format!("local:{stem}"),
            title: stem,
            repo_name: String::new(),
            layout: "*".to_string(),
            path,
            course_file: file_name.clone(),
            custom: custom.contains(&file_name),
        });
    }
    // Repo packs.
    let repos_root = crate::paths::repos_dir(root);
    for url in urls {
        let slug = slug_for_url(url);
        let dir = repos_root.join(&slug);
        if !dir.is_dir() {
            continue;
        }
        let details = details_with_fallback(&dir, &slug);
        let repo_name = if details.name.is_empty() { slug.clone() } else { details.name.clone() };
        for entry in &details.courses {
            if !layout_matches(&entry.layout, layout_filter) {
                continue;
            }
            let abs = dir.join(&entry.path);
            if !abs.is_file() {
                continue;
            }
            let stem = entry
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| {
                    abs.file_stem().unwrap_or_default().to_string_lossy().to_string()
                });
            if stem.is_empty() {
                continue;
            }
            let file_name = abs.file_name().unwrap_or_default().to_string_lossy().to_string();
            out.push(AvailableCourse {
                id: format!("repo:{slug}:{}:{stem}", entry.layout),
                title: stem,
                repo_name: repo_name.clone(),
                layout: entry.layout.clone(),
                path: abs,
                course_file: format!("{slug}:{}:{file_name}", entry.layout),
                custom: false,
            });
        }
    }
    out.sort_by_key(|c| (c.title.to_lowercase(), c.repo_name.to_lowercase(), c.id.clone()));
    out
}

fn layout_matches(entry_layout: &str, filter: &str) -> bool {
    if filter.trim().is_empty() || entry_layout == "*" {
        return true;
    }
    entry_layout.eq_ignore_ascii_case(filter.trim())
}

/// Next numbered lesson inside the same pack+layout (repo-aware successor of
/// `courses::next_lesson`). Local lessons keep the legacy behaviour.
pub fn next_course(available: &[AvailableCourse], current_id: &str) -> Option<AvailableCourse> {
    let current = available.iter().find(|c| c.id == current_id)?;
    if current.custom {
        return None;
    }
    let current_number = leading_number(&current.title)?;
    let same_pack: Vec<&AvailableCourse> = available
        .iter()
        .filter(|c| {
            !c.custom
                && c.repo_name == current.repo_name
                && c.layout == current.layout
                && c.id != current.id
        })
        .collect();
    let mut best: Option<&AvailableCourse> = None;
    for cand in same_pack {
        let Some(n) = leading_number(&cand.title) else { continue };
        if n > current_number {
            let replace = match best {
                None => true,
                Some(b) => {
                    let bn = leading_number(&b.title).unwrap_or(u64::MAX);
                    n < bn || (n == bn && cand.title.to_lowercase() < b.title.to_lowercase())
                }
            };
            if replace {
                best = Some(cand);
            }
        }
    }
    best.cloned()
}

fn leading_number(stem: &str) -> Option<u64> {
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() { None } else { digits.parse().ok() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_repo(dir: &Path, details: &str, files: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("details.json"), details).unwrap();
        for (rel, content) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, content).unwrap();
        }
    }

    #[test]
    fn details_map_form() {
        let d = parse_details(r#"{"name":"Main","creator":"bit","courses":{"HU-qwertz":["HU-qwertz/a.txt"],"US-qwerty":["US-qwerty/b.txt"]}}"#).unwrap();
        assert_eq!(d.name, "Main");
        assert_eq!(d.creator, "bit");
        assert_eq!(d.courses.len(), 2);
    }

    #[test]
    fn details_list_form() {
        let d = parse_details(r#"{"name":"N","creator":"C","courses":[{"layout":"HU-qwertz","path":"HU-qwertz/a.txt","title":"Intro"}]}"#).unwrap();
        assert_eq!(d.courses[0].title.as_deref(), Some("Intro"));
    }

    #[test]
    fn slug_and_default() {
        assert_eq!(slug_for_url("https://github.com/bitiskola/bit-typing-main"), "bit-typing-main");
        assert_eq!(slug_for_url("https://github.com/x/y.git/"), "y");
        // Windows local paths (backslashes) slug to their last segment too.
        assert_eq!(slug_for_url(r"C:\Users\me\AppData\Local\Temp\pack-src"), "pack-src");
        assert_eq!(slug_for_url(r"C:\repos\pack.git"), "pack");
        assert!(is_default_repo("https://github.com/bitiskola/bit-typing-main.git"));
        let mut v = vec!["https://example.com/other".to_string()];
        ensure_default_repo(&mut v);
        assert_eq!(v[0], DEFAULT_REPO_URL);
    }

    #[test]
    fn collect_filters_by_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(crate::paths::courses_dir(root)).unwrap();
        std::fs::create_dir_all(crate::paths::repos_dir(root)).unwrap();
        std::fs::write(crate::paths::courses_dir(root).join("local.txt"), "hello").unwrap();
        let slug = slug_for_url("https://example.com/pack");
        write_repo(
            &crate::paths::repos_dir(root).join(&slug),
            r#"{"name":"Pack","creator":"me","courses":{"HU-qwertz":["HU-qwertz/hu.txt"],"US-qwerty":["US-qwerty/us.txt"]}}"#,
            &[("HU-qwertz/hu.txt", "szia"), ("US-qwerty/us.txt", "hi")],
        );
        let urls = vec!["https://example.com/pack".to_string()];
        let custom = BTreeSet::new();
        let hu = collect_available_courses(root, &urls, "HU-qwertz", &custom);
        assert!(hu.iter().any(|c| c.title == "local"));
        assert!(hu.iter().any(|c| c.title == "hu"));
        assert!(!hu.iter().any(|c| c.title == "us"));
        let us = collect_available_courses(root, &urls, "US-qwerty", &custom);
        assert!(us.iter().any(|c| c.title == "us"));
        assert!(!us.iter().any(|c| c.title == "hu"));
        // Two repos with the same file name both load (distinct ids).
        let slug2 = slug_for_url("https://example.com/other");
        write_repo(
            &crate::paths::repos_dir(root).join(&slug2),
            r#"{"name":"Other","creator":"you","courses":{"HU-qwertz":["HU-qwertz/hu.txt"]}}"#,
            &[("HU-qwertz/hu.txt", "hello again")],
        );
        let both = collect_available_courses(root, &["https://example.com/pack".to_string(), "https://example.com/other".to_string()], "HU-qwertz", &custom);
        assert_eq!(both.iter().filter(|c| c.title == "hu").count(), 2);
    }

    #[test]
    fn fallback_scan_without_details() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("pack");
        std::fs::create_dir_all(dir.join("HU-qwertz")).unwrap();
        std::fs::write(dir.join("HU-qwertz/a.txt"), "x").unwrap();
        let d = details_with_fallback(&dir, "pack");
        assert_eq!(d.courses.len(), 1);
        assert_eq!(d.courses[0].layout, "HU-qwertz");
    }

    #[test]
    fn next_course_same_pack_only() {        let mk = |id: &str, title: &str, repo: &str, layout: &str| AvailableCourse {
            id: id.to_string(), title: title.to_string(), repo_name: repo.to_string(),
            layout: layout.to_string(), path: PathBuf::from(id), course_file: id.to_string(), custom: false,
        };
        let avail = vec![
            mk("a", "01", "Pack", "HU-qwertz"),
            mk("b", "02", "Pack", "HU-qwertz"),
            mk("c", "02", "Other", "HU-qwertz"),
        ];
        let next = next_course(&avail, "a").unwrap();
        assert_eq!(next.id, "b");
        assert!(next_course(&avail, "b").is_none());
    }

    #[test]
    fn git_program_prefers_system_git() {
        let system = Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        match git_program() {
            // System git wins so user config/proxies keep working.
            Some(p) if system => assert_eq!(p, PathBuf::from("git")),
            // No system git: bundled MinGit (or nothing) is acceptable.
            Some(_) => {}
            None => assert!(!system, "system git works but nothing resolved"),
        }
    }

    /// Regression test for "fatal: not a git repository" sticking forever:
    /// a `.git` dir git rejects (interrupted clone, foreign files) is
    /// recovered via temp-dir re-clone instead of erroring every refresh.
    #[test]
    fn corrupt_checkout_recovers_via_reclone() {
        let git_ok = Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(git_ok, "test requires git on PATH");
        let tmp = tempfile::tempdir().unwrap();
        // Source repo with one course (local path = offline-capable "url").
        let src = tmp.path().join("pack-src");
        std::fs::create_dir_all(src.join("HU-qwertz")).unwrap();
        std::fs::write(
            src.join("details.json"),
            r#"{"name":"Pack","creator":"me","courses":{"HU-qwertz":["HU-qwertz/a.txt"]}}"#,
        )
        .unwrap();
        std::fs::write(src.join("HU-qwertz/a.txt"), "hello").unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git").args(args).current_dir(&src).output().unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["add", "-A"]);
        git(&["-c", "user.name=test", "-c", "user.email=test@test", "commit", "-m", "init"]);
        let repos_root = tmp.path().join("repos");
        let url = src.to_string_lossy().to_string();
        // First refresh clones.
        let d = refresh_repo(&repos_root, &url).unwrap();
        assert_eq!(d.courses.len(), 1);
        let dest = repos_root.join("pack-src");
        assert!(dest.join(".git").is_dir());
        // Simulate the reported failure: a `.git` dir git rejects.
        std::fs::remove_dir_all(dest.join(".git")).unwrap();
        std::fs::create_dir_all(dest.join(".git")).unwrap();
        // Recovery re-clones from the source; courses work again.
        let d2 = refresh_repo(&repos_root, &url).unwrap();
        assert_eq!(d2.courses.len(), 1);
        assert!(dest.join(".git").join("HEAD").is_file());
        assert!(dest.join("HU-qwertz").join("a.txt").is_file());
        // No temp leftovers leak into the repos root.
        let leftovers: Vec<_> = std::fs::read_dir(&repos_root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "temp dirs leaked: {leftovers:?}");
    }
}
