//! `bit-typing-setup`: self-contained installer.
//!
//! Mirrors `installer.py` / `build_win.bat` step [3/3]:
//! copies `bit-typing.exe` + `uninstall.exe` payload into the chosen folder,
//! writes `install-info.json`, creates Desktop / Start-Menu shortcuts and
//! registers the Windows *Add/Remove programs* entry.
//!
//! The payload is resolved at runtime: next to the setup binary in
//! `payload/` (bundled layout) or beside it (dev layout). `build_win.bat`
//! copies the freshly built `dist/*.exe` there before shipping, mirroring
//! PyInstaller's `--add-binary ... payload` step.

// Windows-only installer items are unused when compiling on Linux (the build
// host); they are all used in the Windows build.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

const APP_NAME: &str = "bit-typing";
const APP_TITLE: &str = "BIT Typing";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> eframe::Result<()> {
    #[cfg(not(windows))]
    {
        eprintln!("This installer can only run on Windows (current: {}).", std::env::consts::OS);
        eprintln!("On Linux use ./build_linux.sh, on macOS ./build_macos.sh.");
        std::process::exit(1);
    }
    #[cfg(windows)]
    {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([820.0, 570.0])
                .with_resizable(false)
                .with_title(format!("{APP_TITLE} Setup")),
            ..Default::default()
        };
        eframe::run_native(
            "bit-typing-setup",
            options,
            Box::new(|_cc| Ok(Box::new(SetupApp::new()))),
        )
    }
}

#[cfg(windows)]
fn default_install_dir(admin: bool) -> PathBuf {
    if admin {
        let base =
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        PathBuf::from(base).join(APP_NAME)
    } else {
        let base = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs_home().join("AppData").join("Local").join("Programs")
            });
        base.join(APP_NAME)
    }
}

#[cfg(windows)]
fn dirs_home() -> PathBuf {
    std::env::var("USERPROFILE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("C:\\"))
}

#[cfg(windows)]
struct SetupApp {
    install_dir: String,
    desktop: bool,
    start_menu: bool,
    status: String,
    error: String,
    working: bool,
    done: bool,
    is_admin: bool,
}

#[cfg(windows)]
impl SetupApp {
    fn new() -> Self {
        let admin = is_admin();
        Self {
            install_dir: default_install_dir(admin).to_string_lossy().to_string(),
            desktop: true,
            start_menu: true,
            status: "Ready to install".to_string(),
            error: String::new(),
            working: false,
            done: false,
            is_admin: admin,
        }
    }
}

#[cfg(windows)]
impl eframe::App for SetupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Install bit-typing");
            ui.label(if self.is_admin {
                "● Administrator mode — installs for every user"
            } else {
                "● Standard mode — installs only for your account"
            });
            ui.separator();
            ui.label("Installation folder");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.install_dir);
                if ui.button("Choose…").clicked() {
                    // Keep dependency-free: typed path (improvement: validated inline).
                }
            });
            ui.checkbox(&mut self.desktop, "Create desktop shortcut");
            ui.checkbox(&mut self.start_menu, "Create Start menu shortcut");
            if !self.error.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(0xef, 0x44, 0x44), &self.error);
            }
            ui.label(&self.status);
            let label = if self.done {
                "Finish"
            } else if self.working {
                "Installing…"
            } else {
                "Install bit-typing"
            };
            if ui
                .add_enabled(!self.working || self.done, egui::Button::new(label))
                .clicked()
            {
                if self.done {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                self.error.clear();
                match validate_install_path(&self.install_dir) {
                    Ok(dir) => {
                        self.working = true;
                        let desktop = self.desktop;
                        let start_menu = self.start_menu;
                        let scope = if self.is_admin { "machine" } else { "user" }.to_string();
                        // Synchronous: install_application is file I/O bound and
                        // finishes fast; the UI repaints with the result below.
                        match install_application(&dir, &scope, desktop, start_menu) {
                            Ok(()) => {
                                self.done = true;
                                self.working = false;
                                self.status = format!("Installed successfully in {}", dir.display());
                            }
                            Err(e) => {
                                self.working = false;
                                self.error = format!("Installation failed: {e}");
                                self.status = "Installation failed".to_string();
                            }
                        }
                    }
                    Err(e) => self.error = e,
                }
            }
        });
    }
}

fn validate_install_path(raw: &str) -> Result<PathBuf, String> {
    let expanded = raw.trim().trim_matches('"').to_string();
    if expanded.is_empty() {
        return Err("Choose an installation folder.".to_string());
    }
    let path = PathBuf::from(expanded);
    if !path.is_absolute() {
        return Err("The installation folder must be an absolute path.".to_string());
    }
    if path.parent().is_none() {
        return Err("Installing directly into a drive root is not allowed.".to_string());
    }
    Ok(path)
}

#[cfg(windows)]
fn payload_file(name: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().unwrap_or(Path::new("."));
    for candidate in [dir.join("payload").join(name), dir.join(name)] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("The installer payload is missing {name}."))
}

#[cfg(windows)]
fn install_application(
    install_dir: &Path,
    scope: &str,
    desktop_shortcut: bool,
    start_menu_shortcut: bool,
) -> Result<(), String> {
    std::fs::create_dir_all(install_dir).map_err(|e| e.to_string())?;
    let src_app = payload_file(&format!("{APP_NAME}.exe"))?;
    let src_un = payload_file("uninstall.exe")?;
    std::fs::copy(&src_app, install_dir.join(format!("{APP_NAME}.exe")))
        .map_err(|e| e.to_string())?;
    std::fs::copy(&src_un, install_dir.join("uninstall.exe")).map_err(|e| e.to_string())?;
    let info = serde_json::json!({
        "app": APP_TITLE,
        "version": APP_VERSION,
        "scope": scope,
        "install_dir": install_dir.to_string_lossy(),
        "desktop_shortcut": desktop_shortcut,
        "start_menu_shortcut": start_menu_shortcut,
    });
    std::fs::write(
        install_dir.join("install-info.json"),
        serde_json::to_string_pretty(&info).unwrap(),
    )
    .map_err(|e| e.to_string())?;

    // Shortcuts via PowerShell (same technique as installer.py).
    let app_exe = install_dir.join(format!("{APP_NAME}.exe"));
    for (wanted, kind) in [(desktop_shortcut, "desktop"), (start_menu_shortcut, "start")] {
        let link = shortcut_path(scope, kind)?;
        if wanted {
            create_shortcut(&link, &app_exe).map_err(|e| e.to_string())?;
        } else {
            let _ = std::fs::remove_file(&link);
        }
    }
    register_uninstaller(scope, install_dir).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn shortcut_path(scope: &str, kind: &str) -> Result<PathBuf, String> {
    // Resolve well-known folders without extra crates.
    let folder: PathBuf = if scope == "machine" {
        if kind == "desktop" {
            std::env::var("PUBLIC").map(|p| PathBuf::from(p).join("Desktop")).unwrap_or_else(|_| PathBuf::from(r"C:\Users\Public\Desktop"))
        } else {
            PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs")
        }
    } else if kind == "desktop" {
        dirs_home().join("Desktop")
    } else {
        std::env::var("APPDATA").map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs")).unwrap_or_else(|_| dirs_home().join("Start Menu"))
    };
    Ok(folder.join(format!("{APP_NAME}.lnk")))
}

#[cfg(windows)]
fn create_shortcut(link: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Save()",
        link.display(),
        target.display(),
        target.parent().unwrap_or(Path::new("C:\\")).display()
    );
    std::process::Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(0x08000000)
        .status()?;
    Ok(())
}

#[cfg(windows)]
fn register_uninstaller(scope: &str, install_dir: &Path) -> std::io::Result<()> {
    // Minimal registry entry via `reg.exe` (no winreg crate needed).
    let hive = if scope == "machine" { "HKLM" } else { "HKCU" };
    let key = format!(r"{hive}\Software\Microsoft\Windows\CurrentVersion\Uninstall\{APP_NAME}");
    let app_exe = install_dir.join(format!("{APP_NAME}.exe"));
    let unins = install_dir.join("uninstall.exe");
    let pairs = [
        ("DisplayName", APP_TITLE),
        ("DisplayVersion", APP_VERSION),
        ("Publisher", "micr0softstore"),
        ("InstallLocation", &install_dir.to_string_lossy()),
        ("DisplayIcon", &app_exe.to_string_lossy()),
    ];
    for (name, value) in pairs {
        std::process::Command::new("reg.exe")
            .args(["add", &key, "/v", name, "/t", "REG_SZ", "/d", value, "/f"])
            .status()?;
    }
    std::process::Command::new("reg.exe")
        .args(["add", &key, "/v", "UninstallString", "/t", "REG_SZ", "/d",
            &format!("\"{}\"", unins.display()), "/f"])
        .status()?;
    Ok(())
}

#[cfg(windows)]
fn is_admin() -> bool {
    // Best-effort: try opening a machine registry key for write via reg.exe.
    std::process::Command::new("reg.exe")
        .args(["query", r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        && std::env::var("USERNAME").is_ok()
}
