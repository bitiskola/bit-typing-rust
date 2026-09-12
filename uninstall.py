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
import time
import uuid
from pathlib import Path
from tkinter import BooleanVar, messagebox
from ctypes import wintypes

import customtkinter as ctk
import tkinter as tk
from PIL import Image, ImageOps


APP_NAME = "bit-typing"
APP_TITLE = "BIT Typing"
APP_VERSION = "2.0.0"
# install-info.json `app` values accepted here: the Python installer writes
# "bit-typing" while the older Rust setup binary wrote "BIT Typing".
# Rejecting either one bricks uninstallation, so both validate.
APP_IDS = ("bit-typing", "BIT Typing")
# Local app-data folder name; must match the Rust app (paths.rs APP_NAME).
APP_DATA_DIR_NAME = "BIT Typing"
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
    "error_hover": "#d93636",
}

CSIDL_PROGRAMS = 0x0002
CSIDL_DESKTOPDIRECTORY = 0x0010
CSIDL_COMMON_PROGRAMS = 0x0017
CSIDL_COMMON_DESKTOPDIRECTORY = 0x0019
CREATE_NO_WINDOW = 0x08000000
CREATE_NEW_PROCESS_GROUP = 0x00000200
DETACHED_PROCESS = 0x00000008


def is_frozen() -> bool:
    return bool(getattr(sys, "frozen", False))


def is_admin() -> bool:
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except (AttributeError, OSError):
        return False


def executable_dir() -> Path:
    if is_frozen():
        return Path(sys.executable).resolve().parent
    return Path(__file__).resolve().parent


def bundle_root() -> Path:
    return Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))


def apply_window_identity(window: ctk.CTk, title: str) -> None:
    window.title(title)
    try:
        window.tk.call("tk", "appname", APP_NAME)
    except tk.TclError:
        pass
    try:
        with Image.open(bundle_root() / "assets" / "favico.png") as source:
            image = ImageOps.fit(source.convert("RGBA"), (256, 256), method=Image.Resampling.LANCZOS)
        png = io.BytesIO()
        image.save(png, format="PNG")
        photo = tk.PhotoImage(data=base64.b64encode(png.getvalue()).decode("ascii"), format="png")
        window._bit_typing_icon_photo = photo
        window.iconphoto(True, photo)
        ico_path = bundle_root() / "assets" / "favico.ico"
        if not ico_path.is_file():
            ico_path = Path(tempfile.gettempdir()) / "bit-typing-uninstall-favico.ico"
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


def load_install_info() -> dict[str, object]:
    install_dir = executable_dir()
    try:
        value = json.loads((install_dir / "install-info.json").read_text(encoding="utf-8"))
        info = value if isinstance(value, dict) else {}
    except (OSError, json.JSONDecodeError):
        info = {}
    info["install_dir"] = str(install_dir)
    if info.get("scope") not in {"machine", "user"}:
        info["scope"] = "machine" if is_admin() else "user"
    return info


def local_data_dir() -> Path:
    base = os.environ.get("LOCALAPPDATA")
    return (Path(base) if base else Path.home() / "AppData" / "Local") / APP_DATA_DIR_NAME


def shell_folder(csidl: int) -> Path:
    buffer = ctypes.create_unicode_buffer(32768)
    result = ctypes.windll.shell32.SHGetFolderPathW(None, csidl, None, 0, buffer)
    if result != 0 or not buffer.value:
        raise OSError(f"Windows could not resolve shell folder {csidl}.")
    return Path(buffer.value)


def shortcut_locations(scope: str, stem: str = APP_NAME) -> tuple[Path, Path]:
    if scope == "machine":
        return (
            shell_folder(CSIDL_COMMON_DESKTOPDIRECTORY) / f"{stem}.lnk",
            shell_folder(CSIDL_COMMON_PROGRAMS) / f"{stem}.lnk",
        )
    return (
        shell_folder(CSIDL_DESKTOPDIRECTORY) / f"{stem}.lnk",
        shell_folder(CSIDL_PROGRAMS) / f"{stem}.lnk",
    )


def remove_shortcuts(scope: str) -> None:
    # Remove both the current ("bit-typing.lnk") and legacy ("BIT Typing.lnk")
    # shortcut names so older installs are fully cleaned up.
    for stem in (APP_NAME, APP_TITLE):
        for shortcut in shortcut_locations(scope, stem):
            try:
                shortcut.unlink(missing_ok=True)
            except OSError:
                pass


def remove_registry_entry(scope: str) -> None:
    import winreg

    hive = winreg.HKEY_LOCAL_MACHINE if scope == "machine" else winreg.HKEY_CURRENT_USER
    # Same story as shortcuts: the entry may live under either name.
    for stem in (APP_NAME, APP_TITLE):
        try:
            winreg.DeleteKey(hive, rf"Software\Microsoft\Windows\CurrentVersion\Uninstall\{stem}")
        except FileNotFoundError:
            pass


def validate_installed_directory(path: Path) -> None:
    if path.parent == path:
        raise ValueError("Refusing to remove a drive root.")
    marker = path / "install-info.json"
    try:
        info = json.loads(marker.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("The bit-typing installation marker is missing or invalid.") from error
    if not isinstance(info, dict) or info.get("app") not in APP_IDS:
        raise ValueError("The selected directory is not a bit-typing installation.")
    if not (path / f"{APP_NAME}.exe").is_file():
        raise ValueError("The bit-typing application executable is missing.")


def terminate_running_application() -> None:
    subprocess.run(
        ["taskkill.exe", "/F", "/T", "/IM", f"{APP_NAME}.exe"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        creationflags=CREATE_NO_WINDOW,
        check=False,
    )


def relaunch_as_admin(arguments: list[str]) -> bool:
    if is_admin():
        return True
    executable = Path(sys.executable).resolve()
    launch_arguments = list(arguments)
    if not is_frozen():
        launch_arguments.insert(0, str(Path(__file__).resolve()))
    parameters = subprocess.list2cmdline(launch_arguments)
    shell_execute = ctypes.windll.shell32.ShellExecuteW
    shell_execute.argtypes = (
        wintypes.HWND,
        wintypes.LPCWSTR,
        wintypes.LPCWSTR,
        wintypes.LPCWSTR,
        wintypes.LPCWSTR,
        ctypes.c_int,
    )
    shell_execute.restype = wintypes.HINSTANCE
    result = shell_execute(
        None, "runas", str(executable), parameters, str(executable.parent), 1
    )
    if not result or result <= 32:
        raise PermissionError("Administrator permission is required to remove this installation.")
    return False


def wait_for_process(process_id: int) -> None:
    synchronize = 0x00100000
    infinite = 0xFFFFFFFF
    kernel32 = ctypes.windll.kernel32
    kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    kernel32.WaitForSingleObject.restype = wintypes.DWORD
    kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel32.CloseHandle.restype = wintypes.BOOL
    handle = kernel32.OpenProcess(synchronize, False, process_id)
    if handle:
        try:
            kernel32.WaitForSingleObject(handle, infinite)
        finally:
            kernel32.CloseHandle(handle)
    else:
        time.sleep(2)


def schedule_helper_removal() -> None:
    helper = Path(sys.executable).resolve()
    batch = helper.with_suffix(".cmd")
    batch.write_text(
        "@echo off\r\n"
        "for /L %%i in (1,1,30) do (\r\n"
        f'  del /f /q "%~dp0{helper.name}" >nul 2>&1\r\n'
        f'  if not exist "%~dp0{helper.name}" goto removed\r\n'
        "  timeout /t 1 /nobreak >nul\r\n"
        ")\r\n"
        ":removed\r\n"
        'del /f /q "%~f0"\r\n',
        encoding="utf-8",
    )
    subprocess.Popen(
        ["cmd.exe", "/c", str(batch)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        creationflags=CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
        close_fds=True,
    )


def cleanup_worker(parent_pid: int, install_dir: Path, remove_data: bool) -> None:
    # Revalidate in the detached helper as well; command-line arguments must
    # never be sufficient to make the cleanup mode delete an arbitrary folder.
    validate_installed_directory(install_dir)
    wait_for_process(parent_pid)
    removed = False
    for _attempt in range(40):
        try:
            shutil.rmtree(install_dir)
            removed = True
            break
        except FileNotFoundError:
            removed = True
            break
        except OSError:
            time.sleep(0.25)
    if remove_data:
        shutil.rmtree(local_data_dir(), ignore_errors=True)
    if not removed:
        error_file = Path(tempfile.gettempdir()) / "bit-typing-uninstall-error.txt"
        error_file.write_text(
            f"Could not completely remove {install_dir}. Close all bit-typing processes and delete it manually.",
            encoding="utf-8",
        )
    schedule_helper_removal()


def launch_cleanup_helper(install_dir: Path, remove_data: bool) -> None:
    if not is_frozen():
        raise RuntimeError("Compile uninstall.py before using it to remove an installation.")
    helper_dir = Path(tempfile.gettempdir()) / f"bit-typing-uninstall-{uuid.uuid4().hex}"
    helper_dir.mkdir(parents=True)
    helper = helper_dir / "uninstall-cleanup.exe"
    shutil.copy2(sys.executable, helper)
    subprocess.Popen(
        [
            str(helper),
            "--cleanup",
            str(os.getpid()),
            str(install_dir),
            "1" if remove_data else "0",
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        creationflags=CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
        close_fds=True,
    )


def begin_uninstall(info: dict[str, object], remove_data: bool) -> None:
    scope = str(info["scope"])
    install_dir = Path(str(info["install_dir"])).resolve()
    validate_installed_directory(install_dir)
    terminate_running_application()
    remove_shortcuts(scope)
    remove_registry_entry(scope)
    launch_cleanup_helper(install_dir, remove_data)


class UninstallerApp(ctk.CTk):
    def __init__(self, info: dict[str, object]) -> None:
        super().__init__(className=f"{APP_NAME}-uninstall")
        apply_window_identity(self, f"Uninstall {APP_TITLE}")
        self.info = info
        self.working = False
        self.geometry("700x500")
        self.resizable(False, False)
        self.configure(fg_color=COLORS["bg"])
        self._center_window()
        self._build_ui()

    def _center_window(self) -> None:
        self.update_idletasks()
        x = max(0, (self.winfo_screenwidth() - 700) // 2)
        y = max(0, (self.winfo_screenheight() - 500) // 2)
        self.geometry(f"700x500+{x}+{y}")

    def _build_ui(self) -> None:
        header = ctk.CTkFrame(self, height=104, corner_radius=0, fg_color="#232936")
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
        logo.pack(side="left", padx=(28, 16), pady=21)
        title_box = ctk.CTkFrame(header, fg_color="transparent")
        title_box.pack(side="left", pady=19)
        ctk.CTkLabel(
            title_box,
            text="Uninstall bit-typing",
            text_color=COLORS["text"],
            font=ctk.CTkFont(size=27, weight="bold"),
        ).pack(anchor="w")
        ctk.CTkLabel(
            title_box,
            text="Remove the application safely from Windows",
            text_color=COLORS["muted"],
            font=ctk.CTkFont(size=14),
        ).pack(anchor="w", pady=(2, 0))

        body = ctk.CTkFrame(self, fg_color="transparent")
        body.pack(fill="both", expand=True, padx=30, pady=28)
        ctk.CTkLabel(
            body,
            text="Are you sure you want to remove bit-typing?",
            text_color=COLORS["text"],
            font=ctk.CTkFont(size=20, weight="bold"),
        ).pack(anchor="w")
        ctk.CTkLabel(
            body,
            text=f"Installed in: {self.info['install_dir']}",
            text_color=COLORS["muted"],
            font=ctk.CTkFont(size=13),
            wraplength=625,
            justify="left",
        ).pack(anchor="w", pady=(8, 22))

        option_panel = ctk.CTkFrame(body, fg_color=COLORS["panel"], corner_radius=18)
        option_panel.pack(fill="x")
        self.remove_data_var = BooleanVar(value=False)
        ctk.CTkCheckBox(
            option_panel,
            text="Also remove my courses, settings, and statistics",
            variable=self.remove_data_var,
            height=48,
            checkbox_width=26,
            checkbox_height=26,
            corner_radius=7,
            border_width=2,
            fg_color=COLORS["error"],
            hover_color=COLORS["error_hover"],
            border_color=COLORS["muted"],
            text_color=COLORS["text"],
            font=ctk.CTkFont(size=14, weight="bold"),
        ).pack(anchor="w", padx=22, pady=(15, 2))
        ctk.CTkLabel(
            option_panel,
            text="Leave this unchecked to keep your learning progress for a future reinstall.",
            text_color=COLORS["muted"],
            font=ctk.CTkFont(size=12),
        ).pack(anchor="w", padx=(57, 20), pady=(0, 15))

        self.progress = ctk.CTkProgressBar(
            body,
            height=10,
            corner_radius=8,
            fg_color=COLORS["panel2"],
            progress_color=COLORS["error"],
        )
        self.progress.set(0)
        self.progress.pack(fill="x", pady=(25, 12))
        self.status = ctk.CTkLabel(
            body, text="Ready", text_color=COLORS["muted"], font=ctk.CTkFont(size=13)
        )
        self.status.pack(anchor="w")

        buttons = ctk.CTkFrame(body, fg_color="transparent")
        buttons.pack(fill="x", pady=(23, 0))
        self.cancel_button = ctk.CTkButton(
            buttons,
            text="Cancel",
            width=170,
            height=52,
            corner_radius=16,
            fg_color=COLORS["panel2"],
            hover_color=COLORS["border"],
            font=ctk.CTkFont(size=16, weight="bold"),
            command=self.destroy,
        )
        self.cancel_button.pack(side="left")
        self.remove_button = ctk.CTkButton(
            buttons,
            text="Uninstall",
            height=52,
            corner_radius=16,
            fg_color=COLORS["error"],
            hover_color=COLORS["error_hover"],
            font=ctk.CTkFont(size=16, weight="bold"),
            command=self.start_uninstall,
        )
        self.remove_button.pack(side="right", fill="x", expand=True, padx=(12, 0))

    def start_uninstall(self) -> None:
        if self.working:
            return
        if not messagebox.askyesno(
            "Confirm uninstall",
            "bit-typing will be closed if it is running. Continue?",
            parent=self,
        ):
            return
        self.working = True
        self.cancel_button.configure(state="disabled")
        self.remove_button.configure(state="disabled", text="Uninstalling…")
        self.status.configure(text="Removing shortcuts and application files…", text_color=COLORS["error"])
        self.progress.configure(mode="indeterminate")
        self.progress.start()
        remove_data = self.remove_data_var.get()
        thread = threading.Thread(target=self._worker, args=(remove_data,), daemon=True)
        thread.start()

    def _worker(self, remove_data: bool) -> None:
        try:
            begin_uninstall(self.info, remove_data)
        except Exception as error:
            self.after(0, lambda message=str(error): self._finished(False, message))
        else:
            self.after(0, lambda: self._finished(True, ""))

    def _finished(self, success: bool, error: str) -> None:
        self.progress.stop()
        if success:
            self.progress.configure(mode="determinate")
            self.progress.set(1)
            messagebox.showinfo(
                "Uninstall started",
                "bit-typing has been removed. Personal data was kept unless you selected its removal.",
                parent=self,
            )
            self.destroy()
            return
        self.working = False
        self.progress.configure(mode="determinate")
        self.progress.set(0)
        self.cancel_button.configure(state="normal")
        self.remove_button.configure(state="normal", text="Try again")
        self.status.configure(text="Uninstall failed", text_color=COLORS["error"])
        messagebox.showerror("Uninstall failed", error, parent=self)


def main() -> None:
    if not sys.platform.startswith("win"):
        raise SystemExit("This uninstaller can only run on Windows.")

    try:
        set_app_id = ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID
        set_app_id.argtypes = (ctypes.c_wchar_p,)
        set_app_id.restype = ctypes.c_long
        set_app_id(f"bit-typing.uninstaller.{APP_VERSION}")
    except (AttributeError, OSError, TypeError, ValueError):
        pass

    arguments = sys.argv[1:]
    if arguments and arguments[0] == "--cleanup":
        if len(arguments) != 4:
            raise SystemExit(2)
        cleanup_worker(int(arguments[1]), Path(arguments[2]).resolve(), arguments[3] == "1")
        return

    info = load_install_info()
    if info["scope"] == "machine" and not is_admin():
        if not relaunch_as_admin(arguments):
            return

    if "--quiet" in arguments:
        begin_uninstall(info, remove_data=False)
        return

    ctk.set_appearance_mode("dark")
    ctk.set_default_color_theme("blue")
    UninstallerApp(info).mainloop()


if __name__ == "__main__":
    main()
