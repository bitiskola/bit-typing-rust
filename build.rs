//! Build script: inventory default resources and emit a manifest.
//!
//! Mirrors `build_resources.py`: hashes every file under `courses/*.txt`,
//! `lang/*.json` and `keyboards/*.json`, writes
//! `target/.../default-resources.json`, and exposes the manifest path and
//! version to the crate via `env!`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const GROUPS: &[(&str, &str)] = &[
    ("courses", "txt"),
    ("lang", "json"),
    ("keyboards", "json"),
];

fn inventory(project: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    for (folder, ext) in GROUPS {
        let dir = project.join(folder);
        let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("missing {folder}/ dir: {e}"))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == *ext).unwrap_or(false) && p.is_file())
            .collect();
        entries.sort();
        if entries.is_empty() {
            panic!("No default {folder}/*.{ext} files in {}. Build from the complete source folder.", project.display());
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
        "cargo:warning=Bundling {} default courses, languages, and keyboard layouts.",
        files.len()
    );
    println!(
        "cargo:rustc-env=BIT_TYPING_DEFAULT_RESOURCES={}",
        manifest_path.display()
    );
    println!("cargo:rustc-env=BIT_TYPING_RESOURCE_COUNT={}", files.len());
}
