//! `bit-typing` entry point: native egui window.
//!
//! On Windows the GUI subsystem is used so double-clicking the exe does not
//! open an extra console window (output still works when launched from a
//! terminal, e.g. `--version` or `--verify-resources`).

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use bit_typing::app::BitTypingApp;

fn main() -> eframe::Result<()> {
    // Headless verification for the build scripts (no display needed).
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("bit-typing {}", bit_typing::APP_VERSION);
        return Ok(());
    }
    if args.iter().any(|a| a == "--verify-resources") {
        return verify_resources();
    }
    let icon = load_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 850.0])
        .with_min_inner_size([1050.0, 760.0])
        // Matches `StartupWMClass=bit-typing` in the .desktop file so the
        // running window groups under the installed dock icon (no "Unknown").
        .with_app_id("bit-typing")
        .with_title("BIT Typing");
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "BIT Typing",
        options,
        Box::new(|cc| Ok(Box::new(BitTypingApp::new(cc)))),
    )
}

fn load_icon() -> Option<egui::IconData> {
    let bytes = std::fs::read(
        bit_typing::paths::bundle_source_dir().join("assets/favico.png"),
    )
    .ok()
    .or_else(|| {
        bit_typing::EmbeddedAssets::get("assets/favico.png").map(|f| f.data.to_vec())
    })?;
    let image = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let (w, h) = (image.width(), image.height());
    Some(egui::IconData {
        rgba: image.into_raw(),
        width: w,
        height: h,
    })
}

/// Headless check used by `build_linux.sh` / `build_win.bat`:
/// the freshly built binary must see the same default resources as the
/// source tree (mirrors `build_resources.py verify-executable`).
fn verify_resources() -> eframe::Result<()> {
    let project = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected = bit_typing::courses::inventory(&project);
    let manifest = bit_typing::default_resources_manifest();
    let files = manifest
        .get("files")
        .and_then(|f| f.as_object())
        .cloned()
        .unwrap_or_default();
    if expected.len() != files.len() {
        eprintln!(
            "Resource check failed: source has {} files, manifest has {}.",
            expected.len(),
            files.len()
        );
        std::process::exit(1);
    }
    for (name, digest) in &expected {
        match files.get(name) {
            Some(v) if v.as_str() == Some(digest.as_str()) => {}
            _ => {
                eprintln!("Resource check failed: bundled file differs: {name}");
                std::process::exit(1);
            }
        }
    }
    println!("Verified {} default resource files (--verify-resources).", expected.len());
    Ok(())
}
