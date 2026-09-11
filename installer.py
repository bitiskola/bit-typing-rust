from __future__ import annotations

import base64
import ctypes
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from datetime import datetime
from pathlib import Path
from tkinter import BooleanVar, StringVar, filedialog, messagebox

import customtkinter as ctk
import tkinter as tk
from PIL import Image, ImageOps


APP_NAME = "bit-typing"
APP_VERSION = "2.0.0"
PUBLISHER = "micr0softstore"
COLORS = {
    "bg": "#0b1020",
    "panel": "#121a2e",
    "panel2": "#18233c",
    "accent": "#6c63ff",
    "accent_hover": "#574fe0",
    "accent2": "#2dd4bf",
    "text": "#eef2ff",
    "muted": "#93a4c7",
    "border": "#263451",
    "error": "#ef4444",
}

CSIDL_PROGRAMS = 0x0002
CSIDL_DESKTOPDIRECTORY = 0x0010
CSIDL_COMMON_PROGRAMS = 0x0017
CSIDL_COMMON_DESKTOPDIRECTORY = 0x0019
CREATE_NO_WINDOW = 0x08000000


def is_admin() -> bool:
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except (AttributeError, OSError):
        return False


def bundle_root() -> Path:
    return Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))


def apply_window_identity(window: ctk.CTk, title: str) -> None:
    window.title(title)
    try:
        window.tk.call("tk", "appname", APP_NAME)
    except tk.TclError:
        pass
    png_path = bundle_root() / "assets" / "favico.png"
    try:
        with Image.open(png_path) as source:
            image = ImageOps.fit(source.convert("RGBA"), (256, 256), method=Image.Resampling.LANCZOS)
        png_data = io.BytesIO()
        image.save(png_data, format="PNG")
        photo = tk.PhotoImage(data=base64.b64encode(png_data.getvalue()).decode("ascii"), format="png")
        window._bit_typing_icon_photo = photo
        window.iconphoto(True, photo)
        ico_path = bundle_root() / "assets" / "favico.ico"
        if not ico_path.is_file():
            ico_path = Path(tempfile.gettempdir()) / "bit-typing-setup-favico.ico"
            image.save(
                ico_path,
                format="ICO",
                sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
            )
        window.iconbitmap(str(ico_path))
    except (ImportError, OSError, RuntimeError, ValueError, tk.TclError):
        pass


def widget_brand_image(size: int) -> tk.PhotoImage | None:
    try:
        with Image.open(bundle_root() / "assets" / "favico.png") as source:
            image = ImageOps.fit(source.convert("RGBA"), (size, size), method=Image.Resampling.LANCZOS)
        png = io.BytesIO()
        image.save(png, format="PNG")
        return tk.PhotoImage(data=base64.b64encode(png.getvalue()).decode("ascii"), format="png")
    except (ImportError, OSError, RuntimeError, ValueError, tk.TclError):
        return None


def payload_file(name: str) -> Path:
    candidates = (bundle_root() / "payload" / name, bundle_root() / name)
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise FileNotFoundError(f"The installer payload is missing {name}.")


def shell_folder(csidl: int) -> Path:
    buffer = ctypes.create_unicode_buffer(32768)
    result = ctypes.windll.shell32.SHGetFolderPathW(None, csidl, None, 0, buffer)
    if result != 0 or not buffer.value:
        raise OSError(f"Windows could not resolve shell folder {csidl}.")
    return Path(buffer.value)


def shortcut_locations(scope: str) -> tuple[Path, Path]:
    if scope == "machine":
        return (
            shell_folder(CSIDL_COMMON_DESKTOPDIRECTORY) / f"{APP_NAME}.lnk",
            shell_folder(CSIDL_COMMON_PROGRAMS) / f"{APP_NAME}.lnk",
        )
    return (
        shell_folder(CSIDL_DESKTOPDIRECTORY) / f"{APP_NAME}.lnk",
        shell_folder(CSIDL_PROGRAMS) / f"{APP_NAME}.lnk",
    )


def create_shortcut(shortcut_path: Path, target: Path) -> None:
    shortcut_path.parent.mkdir(parents=True, exist_ok=True)
    script_text = """param(
    [string]$ShortcutPath,
    [string]$TargetPath,
    [string]$WorkingDirectory
)
$Shell = New-Object -ComObject WScript.Shell
$Shortcut = $Shell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = $TargetPath
$Shortcut.WorkingDirectory = $WorkingDirectory
$Shortcut.IconLocation = \"$TargetPath,0\"
$Shortcut.Description = \"Modern typing practice application\"
$Shortcut.Save()
"""
    with tempfile.TemporaryDirectory(prefix="bit-typing-shortcut-") as temporary:
        script_path = Path(temporary) / "create-shortcut.ps1"
        script_path.write_text(script_text, encoding="utf-8-sig")
        subprocess.run(
            [
                "powershell.exe",
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(script_path),
                str(shortcut_path),
                str(target),
                str(target.parent),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            creationflags=CREATE_NO_WINDOW,
            text=True,
        )


def replace_file(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.new")
    try:
        shutil.copy2(source, temporary)
        os.replace(temporary, destination)
    finally:
        try:
            temporary.unlink(missing_ok=True)
        except OSError:
            pass


def registry_key(scope: str):
    import winreg

    hive = winreg.HKEY_LOCAL_MACHINE if scope == "machine" else winreg.HKEY_CURRENT_USER
    path = rf"Software\Microsoft\Windows\CurrentVersion\Uninstall\{APP_NAME}"
    return winreg, hive, path


def register_uninstaller(scope: str, install_dir: Path) -> None:
    winreg, hive, key_path = registry_key(scope)
    app_exe = install_dir / f"{APP_NAME}.exe"
    uninstall_exe = install_dir / "uninstall.exe"
    estimated_size = sum(path.stat().st_size for path in install_dir.glob("*") if path.is_file()) // 1024
    with winreg.CreateKeyEx(hive, key_path, 0, winreg.KEY_WRITE) as key:
        strings = {
            "DisplayName": APP_NAME,
            "DisplayVersion": APP_VERSION,
            "Publisher": PUBLISHER,
            "InstallLocation": str(install_dir),
            "DisplayIcon": f'"{app_exe}"',
            "UninstallString": f'"{uninstall_exe}"',
            "QuietUninstallString": f'"{uninstall_exe}" --quiet',
            "InstallDate": datetime.now().strftime("%Y%m%d"),
        }
        for name, value in strings.items():
            winreg.SetValueEx(key, name, 0, winreg.REG_SZ, value)
        winreg.SetValueEx(key, "NoModify", 0, winreg.REG_DWORD, 1)
        winreg.SetValueEx(key, "NoRepair", 0, winreg.REG_DWORD, 1)
        winreg.SetValueEx(key, "EstimatedSize", 0, winreg.REG_DWORD, estimated_size)


def validate_install_path(raw_path: str) -> Path:
    expanded = os.path.expandvars(os.path.expanduser(raw_path.strip().strip('"')))
    if not expanded:
        raise ValueError("Choose an installation folder.")
    path = Path(expanded)
    if not path.is_absolute():
        raise ValueError("The installation folder must be an absolute path.")
    if path.parent == path:
        raise ValueError("Installing directly into a drive root is not allowed.")
    return path


def install_application(
    install_dir: Path,
    scope: str,
    desktop_shortcut: bool,
    start_menu_shortcut: bool,
) -> None:
    source_app = payload_file(f"{APP_NAME}.exe")
    source_uninstaller = payload_file("uninstall.exe")
    install_dir.mkdir(parents=True, exist_ok=True)

    app_exe = install_dir / f"{APP_NAME}.exe"
    replace_file(source_app, app_exe)
    replace_file(source_uninstaller, install_dir / "uninstall.exe")

    info = {
        "app": APP_NAME,
        "version": APP_VERSION,
        "scope": scope,
        "install_dir": str(install_dir),
        "desktop_shortcut": desktop_shortcut,
        "start_menu_shortcut": start_menu_shortcut,
    }
    (install_dir / "install-info.json").write_text(
        json.dumps(info, indent=2, ensure_ascii=False), encoding="utf-8"
    )

    desktop_link, start_link = shortcut_locations(scope)
    for requested, shortcut in (
        (desktop_shortcut, desktop_link),
        (start_menu_shortcut, start_link),
    ):
        if requested:
            create_shortcut(shortcut, app_exe)
        else:
            shortcut.unlink(missing_ok=True)

    register_uninstaller(scope, install_dir)


class InstallerApp(ctk.CTk):
    def __init__(self) -> None:
        super().__init__(className=f"{APP_NAME}-setup")
        apply_window_identity(self, f"{APP_NAME} Setup")
        self.scope = "machine" if is_admin() else "user"
        self.installing = False
        self.geometry("820x570")
        self.resizable(False, False)
        self.configure(fg_color=COLORS["bg"])
        self._center_window()
        self._build_ui()

    def _center_window(self) -> None:
        self.update_idletasks()
        x = max(0, (self.winfo_screenwidth() - 820) // 2)
        y = max(0, (self.winfo_screenheight() - 570) // 2)
        self.geometry(f"820x570+{x}+{y}")

    def _build_ui(self) -> None:
        header = ctk.CTkFrame(self, height=106, corner_radius=0, fg_color="#232936")
        header.pack(fill="x")
        header.pack_propagate(False)

        self.brand_image = widget_brand_image(62)
        if self.brand_image is not None:
            logo = ctk.CTkLabel(header, text="", image=self.brand_image, width=62, height=62)
        else:
            logo = ctk.CTkLabel(
                header,
                text="⌨",
                width=62,
                height=62,
                corner_radius=16,
                fg_color="#123f4a",
                text_color=COLORS["accent2"],
                font=ctk.CTkFont(size=34, weight="bold"),
            )
        logo.pack(side="left", padx=(28, 16), pady=22)
        title_box = ctk.CTkFrame(header, fg_color="transparent")
        title_box.pack(side="left", pady=20)
        ctk.CTkLabel(
            title_box,
            text="Install bit-typing",
            text_color=COLORS["text"],
            font=ctk.CTkFont(size=28, weight="bold"),
        ).pack(anchor="w")
        ctk.CTkLabel(
            title_box,
            text="A modern typing tutor for Windows",
            text_color=COLORS["muted"],
            font=ctk.CTkFont(size=14),
        ).pack(anchor="w", pady=(2, 0))

        body = ctk.CTkFrame(self, fg_color="transparent")
        body.pack(fill="both", expand=True, padx=30, pady=26)

        mode_text = (
            "● Administrator mode — installs for every user"
            if self.scope == "machine"
            else "● Standard mode — installs only for your account"
        )
        ctk.CTkLabel(
            body,
            text=mode_text,
            height=42,
            corner_radius=14,
            fg_color="#12352f" if self.scope == "machine" else COLORS["panel2"],
            text_color=COLORS["accent2"] if self.scope == "machine" else COLORS["muted"],
            font=ctk.CTkFont(size=14, weight="bold"),
        ).pack(fill="x", pady=(0, 22))

        ctk.CTkLabel(
            body,
            text="Installation folder",
            text_color=COLORS["text"],
            font=ctk.CTkFont(size=16, weight="bold"),
        ).pack(anchor="w", padx=4, pady=(0, 8))

        path_row = ctk.CTkFrame(body, fg_color="transparent")
        path_row.pack(fill="x")
        default_path = self._default_install_path()
        self.path_var = StringVar(value=str(default_path))
        self.path_entry = ctk.CTkEntry(
            path_row,
            textvariable=self.path_var,
            height=50,
            corner_radius=14,
            border_width=2,
            border_color=COLORS["border"],
            fg_color=COLORS["panel"],
            text_color=COLORS["text"],
            font=ctk.CTkFont(size=15),
        )
        self.path_entry.pack(side="left", fill="x", expand=True, padx=(0, 10))
        self.choose_button = ctk.CTkButton(
            path_row,
            text="Choose…",
            width=132,
            height=50,
            corner_radius=14,
            fg_color=COLORS["panel2"],
            hover_color=COLORS["border"],
            font=ctk.CTkFont(size=15, weight="bold"),
            command=self.choose_folder,
        )
        self.choose_button.pack(side="right")

        options = ctk.CTkFrame(body, fg_color=COLORS["panel"], corner_radius=18)
        options.pack(fill="x", pady=24)
        self.desktop_var = BooleanVar(value=True)
        self.start_menu_var = BooleanVar(value=True)
        checkbox_options = {
            "height": 42,
            "corner_radius": 7,
            "border_width": 2,
            "checkbox_width": 26,
            "checkbox_height": 26,
            "fg_color": COLORS["accent"],
            "hover_color": COLORS["accent_hover"],
            "border_color": COLORS["muted"],
            "text_color": COLORS["text"],
            "font": ctk.CTkFont(size=15, weight="bold"),
        }
        ctk.CTkCheckBox(
            options, text="Create desktop shortcut", variable=self.desktop_var, **checkbox_options
        ).pack(anchor="w", padx=22, pady=(15, 2))
        ctk.CTkCheckBox(
            options, text="Create Start menu shortcut", variable=self.start_menu_var, **checkbox_options
        ).pack(anchor="w", padx=22, pady=(2, 15))

        self.progress = ctk.CTkProgressBar(
            body,
            height=10,
            corner_radius=8,
            fg_color=COLORS["panel2"],
            progress_color=COLORS["accent2"],
        )
        self.progress.set(0)
        self.progress.pack(fill="x", pady=(2, 10))
        self.status_label = ctk.CTkLabel(
            body,
            text="Ready to install",
            text_color=COLORS["muted"],
            font=ctk.CTkFont(size=13),
        )
        self.status_label.pack(anchor="w", padx=3)

        self.install_button = ctk.CTkButton(
            body,
            text="Install bit-typing",
            height=56,
            corner_radius=17,
            fg_color=COLORS["accent"],
            hover_color=COLORS["accent_hover"],
            text_color=COLORS["text"],
            font=ctk.CTkFont(size=18, weight="bold"),
            command=self.start_install,
        )
        self.install_button.pack(fill="x", pady=(20, 0))

    def _default_install_path(self) -> Path:
        if self.scope == "machine":
            base = os.environ.get("ProgramFiles", r"C:\Program Files")
        else:
            base = os.environ.get("LOCALAPPDATA", str(Path.home() / "AppData" / "Local"))
            base = str(Path(base) / "Programs")
        return Path(base) / APP_NAME

    def choose_folder(self) -> None:
        selected = filedialog.askdirectory(
            parent=self,
            title="Choose installation folder",
            initialdir=str(Path(self.path_var.get()).parent),
        )
        if selected:
            self.path_var.set(str(Path(selected) / APP_NAME))

    def start_install(self) -> None:
        if self.installing:
            return
        try:
            install_dir = validate_install_path(self.path_var.get())
            payload_file(f"{APP_NAME}.exe")
            payload_file("uninstall.exe")
        except (OSError, ValueError) as error:
            messagebox.showerror("Cannot install", str(error), parent=self)
            return

        self.installing = True
        self.path_entry.configure(state="disabled")
        self.choose_button.configure(state="disabled")
        self.install_button.configure(state="disabled", text="Installing…")
        self.status_label.configure(text="Copying application files…", text_color=COLORS["accent2"])
        self.progress.configure(mode="indeterminate")
        self.progress.start()

        thread = threading.Thread(
            target=self._install_worker,
            args=(install_dir, self.desktop_var.get(), self.start_menu_var.get()),
            daemon=True,
        )
        thread.start()

    def _install_worker(self, install_dir: Path, desktop: bool, start_menu: bool) -> None:
        try:
            install_application(install_dir, self.scope, desktop, start_menu)
        except Exception as error:  # Surface filesystem, registry, and PowerShell failures in the UI.
            self.after(0, lambda message=str(error): self._install_finished(False, message, install_dir))
        else:
            self.after(0, lambda: self._install_finished(True, "", install_dir))

    def _install_finished(self, success: bool, error: str, install_dir: Path) -> None:
        self.progress.stop()
        self.progress.configure(mode="determinate")
        self.progress.set(1 if success else 0)
        if success:
            self.status_label.configure(
                text=f"Installed successfully in {install_dir}", text_color=COLORS["accent2"]
            )
            self.install_button.configure(state="normal", text="Finish", command=self.destroy)
            messagebox.showinfo(
                "Installation complete",
                "bit-typing is installed and ready to use.",
                parent=self,
            )
            return

        self.installing = False
        self.path_entry.configure(state="normal")
        self.choose_button.configure(state="normal")
        self.install_button.configure(state="normal", text="Try again")
        self.status_label.configure(text="Installation failed", text_color=COLORS["error"])
        messagebox.showerror(
            "Installation failed",
            f"bit-typing could not be installed.\n\n{error}\n\n"
            "Close bit-typing if it is currently running, then try again.",
            parent=self,
        )


def main() -> None:
    if not sys.platform.startswith("win"):
        raise SystemExit("This installer can only run on Windows.")
    try:
        set_app_id = ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID
        set_app_id.argtypes = (ctypes.c_wchar_p,)
        set_app_id.restype = ctypes.c_long
        set_app_id(f"bit-typing.installer.{APP_VERSION}")
    except (AttributeError, OSError, TypeError, ValueError):
        pass
    ctk.set_appearance_mode("dark")
    ctk.set_default_color_theme("blue")
    InstallerApp().mainloop()


if __name__ == "__main__":
    main()
