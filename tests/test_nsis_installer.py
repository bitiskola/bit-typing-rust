"""Windows installer regressions (headless, no NSIS needed).

Validates installer.nsi + build_win.bat statically: version wiring,
required NSIS machinery, install/uninstall contract (install-info.json,
shortcuts, registry, per-user data option), and that the retired
PyInstaller-based installer system stays gone.

Run: python -m unittest discover -s tests -v
"""
from __future__ import annotations

import re
import struct
import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]
NSI = PROJECT / "installer.nsi"
BAT = PROJECT / "build_win.bat"
CARGO = PROJECT / "Cargo.toml"

RETIRED = (
    "installer.py",
    "uninstall.py",
    "src/bin/bit-typing-setup.rs",
    "src/bin/bit-typing-uninstall.rs",
    "tests/test_uninstaller.py",
)


def read(name: str) -> str:
    return (PROJECT / name).read_text(encoding="utf-8")


def code_lines(name: str) -> str:
    """installer.nsi without `;` comment lines (docs may name directives)."""
    return "\n".join(
        line
        for line in read(name).splitlines()
        if not line.lstrip().startswith(";")
    )


class NsisInstallerTests(unittest.TestCase):
    def test_script_exists(self):
        self.assertTrue(NSI.is_file(), "installer.nsi is missing")

    def test_old_pyinstaller_system_stays_removed(self):
        for relative in RETIRED:
            with self.subTest(file=relative):
                self.assertFalse(
                    (PROJECT / relative).exists(),
                    f"retired installer file resurrected: {relative}",
                )

    def test_cargo_has_no_setup_uninstall_bins(self):
        cargo = read("Cargo.toml")
        self.assertNotIn("bit-typing-setup", cargo)
        self.assertNotIn("bit-typing-uninstall", cargo)
        self.assertIn('name = "bit-typing"', cargo)

    def test_version_wiring(self):
        match = re.search(r'^version = "(\d+\.\d+\.\d+)"', read("Cargo.toml"), re.M)
        self.assertIsNotNone(match, "Cargo.toml has no x.y.z version")
        version = match.group(1)
        bat = read("build_win.bat")
        default = re.search(r'set "PACKAGE_VERSION=%~1"\s*\r?\n\s*if "%PACKAGE_VERSION%"=="" set "PACKAGE_VERSION=([^"]+)"', bat)
        self.assertIsNotNone(default, "build_win.bat has no default PACKAGE_VERSION")
        self.assertEqual(default.group(1), version)
        nsi = read("installer.nsi")
        self.assertIn("!ifndef VERSION", nsi)
        self.assertIn("${VERSION}", nsi)
        self.assertNotIn(version, code_lines("installer.nsi"),
                         "version must come from /DVERSION, not hardcoded")

    def test_build_script_uses_nsis_not_pyinstaller(self):
        bat = read("build_win.bat")
        self.assertIn("makensis", bat)
        self.assertIn("installer.nsi", bat)
        for retired in ("PyInstaller", "pyinstaller", "installer.py", "uninstall.py", "customtkinter"):
            with self.subTest(token=retired):
                self.assertNotIn(retired, bat)

    def test_required_nsis_machinery(self):
        nsi = read("installer.nsi")
        for token in (
            '!include "MUI2.nsh"',
            '!include "MultiUser.nsh"',
            "MULTIUSER_PAGE_INSTALLMODE",
            "!insertmacro MULTIUSER_INIT",
            "!insertmacro MULTIUSER_UNINIT",
            "WriteUninstaller",
            "install-info.json",
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
            "taskkill",
            "LOCALAPPDATA",
            "UninstPage custom",
            "nsDialogs",
            "CreateShortcut",
            "DisplayName",
            "UninstallString",
        ):
            with self.subTest(token=token):
                self.assertIn(token, nsi)

    def test_no_manual_execution_level(self):
        # MultiUser.nsh owns elevation; a manual RequestExecutionLevel breaks it.
        self.assertNotIn("RequestExecutionLevel", code_lines("installer.nsi"))

    def test_output_name_preserved(self):
        nsi = read("installer.nsi")
        self.assertIn("OutFile", nsi)
        self.assertIn("${APP_NAME}-setup.exe", nsi)
        self.assertIn("outputs", nsi)
        self.assertIn("bit-typing-setup.exe", read("build_win.bat"))

    def test_legacy_shortcuts_and_keys_cleaned(self):
        nsi = read("installer.nsi")
        # Both the current (${APP_NAME}) and legacy (${APP_TITLE}) names.
        self.assertIn("${APP_TITLE}.lnk", nsi)
        self.assertIn("Uninstall\\${APP_TITLE}", nsi)

    def test_shipped_exe_icon_is_valid_ico(self):
        ico = PROJECT / "assets" / "favico.ico"
        self.assertTrue(ico.is_file(), "assets/favico.ico is missing")
        data = ico.read_bytes()
        reserved, kind, count = struct.unpack("<HHH", data[:6])
        self.assertEqual((reserved, kind), (0, 1))
        self.assertGreaterEqual(count, 1)
        sizes = set()
        for i in range(count):
            width, height = struct.unpack("<BB", data[6 + 16 * i:8 + 16 * i])
            sizes.add((width or 256, height or 256))
        self.assertIn((256, 256), sizes, f"ico lacks 256px entry: {sorted(sizes)}")


if __name__ == "__main__":
    unittest.main()
