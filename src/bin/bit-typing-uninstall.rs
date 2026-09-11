//! `bit-typing-uninstall`: safe removal tool.
//!
//! Mirrors `uninstall.py`: validates the `install-info.json` marker before
//! deleting anything, stops a running app, removes shortcuts + registry
//! entry, then removes the install dir (plus optional per-user data).

use std::path::{Path, PathBuf};

const APP_TITLE: &str = "BIT Typing";
const APP_NAME: &str = "bit-typing";

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--quiet") {
        return match uninstall(false) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("Uninstall failed: {e}");
                std::process::exit(1);
            }
        };
    }
    #[cfg(not(windows))]
    {
        eprintln!("This uninstaller can only run on Windows.");
        std::process::exit(1);
    }
    #[cfg(windows)]
    {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([700.0, 500.0])
                .with_resizable(false)
                .with_title(format!("Uninstall {APP_TITLE}")),
            ..Default::default()
        };
        eframe::run_native(
            "bit-typing-uninstall",
            options,
            Box::new(|_cc| Ok(Box::new(UninstallApp::new()))),
        )
    }
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn validate_installed_directory(path: &Path) -> Result<serde_json::Value, String> {
    if path.parent().is_none() {
        return Err("Refusing to remove a drive root.".to_string());
    }
    let marker = path.join("install-info.json");
    let text = std::fs::read_to_string(&marker)
        .map_err(|_| "The bit-typing installation marker is missing or invalid.".to_string())?;
    let info: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "The bit-typing installation marker is missing or invalid.".to_string())?;
    if info.get("app").and_then(|a| a.as_str()) != Some(APP_TITLE) {
        return Err("The selected directory is not a bit-typing installation.".to_string());
    }
    if !path.join(format!("{APP_NAME}.exe")).is_file() {
        return Err("The bit-typing application executable is missing.".to_string());
    }
    Ok(info)
}

fn uninstall(remove_data: bool) -> Result<(), String> {
    let dir = exe_dir();
    let _info = validate_installed_directory(&dir)?;

    #[cfg(windows)]
    {
        let scope = _info.get("scope").and_then(|s| s.as_str()).unwrap_or("user");
        // Close a running app first (mirrors taskkill in uninstall.py).
        let _ = std::process::Command::new("taskkill.exe")
            .args(["/F", "/T", "/IM", &format!("{APP_NAME}.exe")])
            .output();
        // Remove shortcuts.
        for kind in ["desktop", "start"] {
            if let Ok(link) = shortcut_path(scope, kind) {
                let _ = std::fs::remove_file(link);
            }
        }
        // Remove registry entry.
        let hive = if scope == "machine" { "HKLM" } else { "HKCU" };
        let key = format!(r"{hive}\Software\Microsoft\Windows\CurrentVersion\Uninstall\{APP_NAME}");
        let _ = std::process::Command::new("reg.exe").args(["delete", &key, "/f"]).output();
    }

    // Remove install dir (retry briefly: the EXE may still be unloading).
    for _ in 0..40 {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(250)),
        }
    }
    if remove_data {
        let data = local_data_dir();
        let _ = std::fs::remove_dir_all(data);
    }
    Ok(())
}

fn local_data_dir() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(APP_TITLE)
}

#[cfg(windows)]
fn shortcut_path(scope: &str, kind: &str) -> Result<PathBuf, String> {
    let home: PathBuf = std::env::var("USERPROFILE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("C:\\"));
    let folder: PathBuf = if scope == "machine" {
        if kind == "desktop" {
            std::env::var("PUBLIC").map(|p| PathBuf::from(p).join("Desktop")).unwrap_or_else(|_| PathBuf::from(r"C:\Users\Public\Desktop"))
        } else {
            PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs")
        }
    } else if kind == "desktop" {
        home.join("Desktop")
    } else {
        std::env::var("APPDATA").map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs")).unwrap_or_else(|_| home.join("Start Menu"))
    };
    Ok(folder.join(format!("{APP_NAME}.lnk")))
}

#[cfg(windows)]
struct UninstallApp {
    info: String,
    remove_data: bool,
    status: String,
    working: bool,
}

#[cfg(windows)]
impl UninstallApp {
    fn new() -> Self {
        let dir = exe_dir();
        let info = dir.to_string_lossy().to_string();
        Self { info, remove_data: false, status: "Ready".to_string(), working: false }
    }
}

#[cfg(windows)]
impl eframe::App for UninstallApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Uninstall bit-typing");
            ui.label(format!("Installed in: {}", self.info));
            ui.checkbox(&mut self.remove_data, "Also remove my courses, settings, and statistics");
            ui.label("Leave this unchecked to keep your learning progress for a future reinstall.");
            ui.label(&self.status);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui
                    .add_enabled(
                        !self.working,
                        egui::Button::new("Uninstall").fill(egui::Color32::from_rgb(0xef, 0x44, 0x44)),
                    )
                    .clicked()
                {
                    self.working = true;
                    self.status = "Removing…".to_string();
                    match uninstall(self.remove_data) {
                        Ok(()) => {
                            self.status = "Removed. This window can be closed.".to_string();
                        }
                        Err(e) => {
                            self.status = format!("Uninstall failed: {e}");
                            self.working = false;
                        }
                    }
                }
            });
        });
    }
}
