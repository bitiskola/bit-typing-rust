"""Uninstaller validation regressions (headless, no display needed).

Run: python -m unittest discover -s tests -v
"""
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

try:
    import uninstall

    HAS_DEPS = True
except ImportError:
    uninstall = None  # type: ignore[assignment]
    HAS_DEPS = False


@unittest.skipUnless(HAS_DEPS, "tkinter/customtkinter required")
class ValidateInstalledDirectoryTests(unittest.TestCase):
    def make_install(self, directory: Path, app_value: str) -> Path:
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "install-info.json").write_text(
            json.dumps({"app": app_value}), encoding="utf-8"
        )
        (directory / "bit-typing.exe").write_bytes(b"MZ")
        return directory

    def test_python_installer_layout_validates(self):
        # Regression: installer.py writes app="bit-typing" while the
        # uninstaller used to demand "BIT Typing", rejecting every such
        # install with "not a bit-typing installation".
        with tempfile.TemporaryDirectory() as tmp:
            directory = self.make_install(Path(tmp) / "bit-typing", "bit-typing")
            uninstall.validate_installed_directory(directory)  # must not raise

    def test_rust_setup_layout_validates(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = self.make_install(Path(tmp) / "bit-typing", "BIT Typing")
            uninstall.validate_installed_directory(directory)  # must not raise

    def test_unknown_layout_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = self.make_install(Path(tmp) / "other", "something-else")
            with self.assertRaises(ValueError):
                uninstall.validate_installed_directory(directory)

    def test_missing_marker_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp) / "bit-typing"
            directory.mkdir()
            (directory / "bit-typing.exe").write_bytes(b"MZ")
            with self.assertRaises(ValueError):
                uninstall.validate_installed_directory(directory)

    def test_missing_exe_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp) / "bit-typing"
            directory.mkdir()
            (directory / "install-info.json").write_text(
                json.dumps({"app": "bit-typing"}), encoding="utf-8"
            )
            with self.assertRaises(ValueError):
                uninstall.validate_installed_directory(directory)

    def test_paths_with_spaces_validate(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = self.make_install(
                Path(tmp) / "Microsoft Store" / "bit-typing", "bit-typing"
            )
            uninstall.validate_installed_directory(directory)  # must not raise


if __name__ == "__main__":
    unittest.main()
