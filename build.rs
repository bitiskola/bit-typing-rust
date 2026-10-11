//! Build script: inventory default resources and emit a manifest.
//!
//! Lessons are no longer bundled: courses come from git course packs
//! (see `src/repos.rs`, `details.json`) and per-user `courses/*.txt` files.
//! The manifest therefore covers `courses/*.txt` only when present, and
//! always covers `lang/*.json` and `keyboards/*.json`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

// `required=false` for courses: an empty/missing courses dir is a valid
// repo-less checkout (lessons arrive via course repos at runtime).
const GROUPS: &[(&str, &str, bool)] = &[
    ("courses", "txt", false),
    ("lang", "json", true),
    ("keyboards", "json", true),
];

fn inventory(project: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    for (folder, ext, required) in GROUPS {
        let dir = project.join(folder);
        let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().map(|x| x == *ext).unwrap_or(false) && p.is_file())
                    .collect()
            })
            .unwrap_or_default();
        entries.sort();
        if entries.is_empty() {
            if *required {
                panic!("No default {folder}/*.{ext} files in {}. Build from the complete source folder.", project.display());
            }
            continue;
        }
        for path in entries {
            let bytes = fs::read(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let digest = hex_encode(hasher.finalize());
            let rel = path
                .strip_prefix(project)
                .expect("resource outside project")
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(rel, digest);
        }
    }
    files
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn main() {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    println!("cargo:rerun-if-changed=courses");
    println!("cargo:rerun-if-changed=lang");
    println!("cargo:rerun-if-changed=keyboards");
    println!("cargo:rerun-if-changed=data/sounds");
    println!("cargo:rerun-if-changed=assets/favico.ico");
    println!("cargo:rerun-if-env-changed=BIT_TYPING_FAVICO");

    let files = inventory(&project);
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_path = out_dir.join("default-resources.json");
    let manifest = serde_json::json!({ "version": 1, "files": files });
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("manifest json"),
    )
    .expect("write manifest");
    println!(
        "cargo:warning=Bundling {} default resource files (courses come from repos, not the bundle).",
        files.len()
    );
    println!(
        "cargo:rustc-env=BIT_TYPING_DEFAULT_RESOURCES={}",
        manifest_path.display()
    );
    println!(
        "cargo:rustc-env=BIT_TYPING_RESOURCE_COUNT={}",
        files.len()
    );
    embed_windows_icon();
}

/// Embed the application icon into Windows executables from the shipped
/// `assets/favico.ico` (`BIT_TYPING_FAVICO` overrides it, as `build_win.bat`
/// does). A missing icon only warns — Linux/macOS and icon-less builds are
/// unaffected.
fn embed_windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = std::env::var("BIT_TYPING_FAVICO").unwrap_or_else(|_| "assets/favico.ico".into());
    if !Path::new(&icon).is_file() {
        println!(
            "cargo:warning=Windows EXE icon skipped (missing {icon}); generate it from assets/favico.png first."
        );
        return;
    }
    let mut res = winres::WindowsResource::new();
    res.set_icon(&icon);
    if let Err(e) = res.compile() {
        println!("cargo:warning=Windows EXE icon skipped ({e}); rc.exe/windres is required for icons.");
    }
}
