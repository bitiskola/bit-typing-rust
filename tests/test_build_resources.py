"""Headless regressions for the defaults embedded in release packages."""
from __future__ import annotations

from contextlib import chdir
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from build_resources import inventory, pyinstaller_data, verify_app, verify_installer, verify_reader

try:
    from PyInstaller.archive.readers import CArchiveReader
    from PyInstaller.archive.writers import CArchiveWriter
except ImportError:
    CArchiveReader = CArchiveWriter = None


class FakeReader:
    """Minimal stand-in for PyInstaller's one-file archive reader."""

    def __init__(self, entries: dict[str, bytes]):
        self.entries = entries
        self.toc = dict.fromkeys(entries)

    def extract(self, name: str) -> bytes:
        return self.entries[name]


class BuildResourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="bit-typing-bundle-test-")
        self.addCleanup(self.temporary.cleanup)
        self.project = Path(self.temporary.name) / "project with spaces"
        self.payloads = {
            "courses/01.txt": "f j f j\nékezetes őű\n".encode("utf-8"),
            "courses/02.txt": b"the next lesson\n",
            "lang/EN-us.json": b'{"name": "English", "strings": {}}',
            "keyboards/US-qwerty.json": b'{"name": "US QWERTY", "rows": []}',
        }
        for relative, contents in self.payloads.items():
            path = self.project / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(contents)
        self.expected = {
            name: hashlib.sha256(contents).hexdigest()
            for name, contents in self.payloads.items()
        }

    def archive_entries(self):
        return {
            **self.payloads,
            "default-resources.json": json.dumps(
                {"version": 1, "files": self.expected}
            ).encode("utf-8"),
        }

    def app_fixture(self):
        app = Path(self.temporary.name) / "bit-typing.app"
        resources = app / "Contents" / "Resources"
        for relative, contents in self.archive_entries().items():
            path = resources / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(contents)
        return app, resources

    def test_inventory_hashes_exact_resource_bytes(self):
        self.assertEqual(inventory(self.project), self.expected)

    def test_every_default_group_must_be_nonempty(self):
        for directory, pattern in (
            ("courses", "*.txt"),
            ("lang", "*.json"),
            ("keyboards", "*.json"),
        ):
            with self.subTest(directory=directory):
                saved = {
                    path: path.read_bytes()
                    for path in (self.project / directory).glob(pattern)
                }
                for path in saved:
                    path.unlink()
                try:
                    with self.assertRaises(ValueError):
                        inventory(self.project)
                finally:
                    for path, contents in saved.items():
                        path.write_bytes(contents)

    def test_missing_resource_directory_is_rejected(self):
        language = self.project / "lang" / "EN-us.json"
        language.unlink()
        language.parent.rmdir()
        with self.assertRaises(ValueError):
            inventory(self.project)

    def test_inventory_does_not_collect_userdata_or_unrelated_files(self):
        unrelated = (
            "data/settings.json", "data/history.json", "data/custom_courses.json",
            "courses/notes.json", "courses/.private", "courses/nested/private.txt",
            "lang/translator-notes.txt", "keyboards/readme.txt",
        )
        for relative in unrelated:
            path = self.project / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"private test data")
        self.assertEqual(inventory(self.project), self.expected)
        collected = {Path(source).resolve() for source, _ in pyinstaller_data(self.project)}
        for relative in unrelated:
            self.assertNotIn((self.project / relative).resolve(), collected)

    def test_data_entries_are_absolute_and_independent_of_working_directory(self):
        unrelated_directory = Path(self.temporary.name) / "elsewhere"
        unrelated_directory.mkdir()
        with chdir(unrelated_directory):
            self.assertEqual(inventory(self.project), self.expected)
            entries = pyinstaller_data(self.project)
        self.assertTrue(all(Path(source).is_absolute() for source, _ in entries))
        for relative in self.payloads:
            self.assertIn(
                (str(self.project / relative), str(Path(relative).parent)), entries
            )
        manifest = self.project / "build-resources" / "default-resources.json"
        self.assertIn((str(manifest), "."), entries)
        self.assertEqual(
            json.loads(manifest.read_text(encoding="utf-8")),
            {"version": 1, "files": self.expected},
        )

    def test_archive_verification_accepts_matching_payload(self):
        verify_reader(FakeReader(self.archive_entries()), self.expected)

    def test_archive_verification_accepts_windows_entry_separators(self):
        entries = {
            name.replace("/", "\\"): contents
            for name, contents in self.archive_entries().items()
        }
        verify_reader(FakeReader(entries), self.expected)

    def test_archive_verification_rejects_missing_or_corrupt_resources(self):
        for name in self.payloads:
            for corrupt in (False, True):
                with self.subTest(name=name, corrupt=corrupt):
                    entries = self.archive_entries()
                    if corrupt:
                        entries[name] = b"not the original bytes"
                    else:
                        del entries[name]
                    with self.assertRaises(ValueError):
                        verify_reader(FakeReader(entries), self.expected)

    def test_archive_verification_rejects_missing_invalid_or_wrong_manifest(self):
        manifests = (
            None,
            b"not json",
            json.dumps({"version": 1, "files": {}}).encode("utf-8"),
            json.dumps({"version": 1, "files": {
                name: "0" * 64 for name in self.expected
            }}).encode("utf-8"),
        )
        for manifest in manifests:
            with self.subTest(manifest=manifest):
                entries = self.archive_entries()
                if manifest is None:
                    del entries["default-resources.json"]
                else:
                    entries["default-resources.json"] = manifest
                with self.assertRaises(ValueError):
                    verify_reader(FakeReader(entries), self.expected)

    def test_mac_app_resources_verify_without_external_resource_folders(self):
        app, _ = self.app_fixture()
        verify_app(app, self.expected)

    def test_mac_app_missing_resource_is_rejected(self):
        app, resources = self.app_fixture()
        (resources / "lang" / "EN-us.json").unlink()
        with self.assertRaises(ValueError):
            verify_app(app, self.expected)

    def test_mac_app_corrupt_resource_is_rejected(self):
        app, resources = self.app_fixture()
        (resources / "courses" / "01.txt").write_bytes(b"corrupted")
        with self.assertRaises(ValueError):
            verify_app(app, self.expected)

    def real_archive(self, filename, entries, binary=False):
        """Produce genuine CArchive bytes, with no executable bootloader/run."""
        directory = Path(self.temporary.name) / "synthetic archives" / filename
        directory.mkdir(parents=True, exist_ok=True)
        toc = []
        for index, (name, contents) in enumerate(entries.items()):
            source = directory / f"source-{index}.bin"
            source.write_bytes(contents)
            toc.append((name, str(source), True, "b" if binary else "x"))
        archive = directory / filename
        CArchiveWriter(str(archive), toc, "python312.dll")
        return archive

    @unittest.skipUnless(CArchiveWriter is not None, "Requires optional PyInstaller build dependency.")
    def test_real_nested_installer_archive_verifies_embedded_defaults(self):
        for separator in ("/", "\\"):
            with self.subTest(separator=separator):
                inner_entries = {
                    name.replace("/", separator): contents
                    for name, contents in self.archive_entries().items()
                }
                inner = self.real_archive("bit-typing.exe", inner_entries)
                payload_name = f"payload{separator}bit-typing.exe"
                outer = self.real_archive(
                    "bit-typing-setup.exe", {payload_name: inner.read_bytes()}, binary=True
                )
                # Verify extraction with the actual reader before exercising
                # the installer's nested-archive verification code path.
                outer_reader = CArchiveReader(str(outer))
                archive_name = next(iter(outer_reader.toc))
                self.assertEqual(outer_reader.extract(archive_name), inner.read_bytes())
                verify_installer(outer, self.expected)

    @unittest.skipUnless(CArchiveWriter is not None, "Requires optional PyInstaller build dependency.")
    def test_real_nested_installer_rejects_missing_or_corrupt_inner_default(self):
        for corrupt in (False, True):
            with self.subTest(corrupt=corrupt):
                entries = self.archive_entries()
                if corrupt:
                    entries["courses/01.txt"] = b"unexpected inner course bytes"
                else:
                    del entries["courses/01.txt"]
                inner = self.real_archive("bit-typing.exe", entries)
                outer = self.real_archive(
                    "bit-typing-setup.exe", {"payload/bit-typing.exe": inner.read_bytes()}, binary=True
                )
                with self.assertRaises(ValueError):
                    verify_installer(outer, self.expected)


if __name__ == "__main__":
    unittest.main()
