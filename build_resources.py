"""Collect and verify defaults in release artifacts; never bundle user history."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import tempfile


MANIFEST = "default-resources.json"
GROUPS = (("courses", "*.txt"), ("lang", "*.json"), ("keyboards", "*.json"))


def inventory(project: Path) -> dict[str, str]:
    project = project.resolve()
    files = {}
    for folder, pattern in GROUPS:
        sources = sorted(path for path in (project / folder).glob(pattern) if path.is_file())
        if not sources:
            raise ValueError(f"No default {folder}/{pattern} files in {project}. Build from the complete source folder.")
        for source in sources:
            files[source.relative_to(project).as_posix()] = hashlib.sha256(source.read_bytes()).hexdigest()
    return files


def pyinstaller_data(project: Path) -> list[tuple[str, str]]:
    project = project.resolve()
    files = inventory(project)
    generated = project / "build-resources" / MANIFEST
    generated.parent.mkdir(parents=True, exist_ok=True)
    generated.write_text(json.dumps({"version": 1, "files": files}, indent=2), encoding="utf-8")
    data = [(str(project / name), str(Path(name).parent)) for name in files]
    data.append((str(generated), "."))
    print(f"Bundling {len(files)} default courses, languages, and keyboard layouts.")
    return data


def _check_manifest(data: bytes, expected: dict[str, str]) -> None:
    manifest = json.loads(data.decode("utf-8"))
    if not isinstance(manifest, dict) or manifest.get("version") != 1 or manifest.get("files") != expected:
        raise ValueError("Bundled default-resource manifest differs from the source files; rebuild the app.")


def verify_reader(reader, expected: dict[str, str]) -> None:
    # PyInstaller archive separators depend on the target platform.
    names = {name.replace("\\", "/"): name for name in reader.toc}
    for name in (MANIFEST, *expected):
        if name not in names:
            raise ValueError(f"Release artifact is missing bundled file: {name}")
    _check_manifest(reader.extract(names[MANIFEST]), expected)
    for name, digest in expected.items():
        if hashlib.sha256(reader.extract(names[name])).hexdigest() != digest:
            raise ValueError(f"Release artifact contains incorrect default file: {name}")


def verify_executable(executable: Path, expected: dict[str, str]) -> None:
    from PyInstaller.archive.readers import CArchiveReader

    verify_reader(CArchiveReader(str(executable)), expected)


def verify_installer(installer: Path, expected: dict[str, str]) -> None:
    from PyInstaller.archive.readers import CArchiveReader

    reader = CArchiveReader(str(installer))
    names = {name.replace("\\", "/"): name for name in reader.toc}
    name = names.get("payload/bit-typing.exe")
    if name is None:
        raise ValueError("Windows installer is missing payload/bit-typing.exe.")
    # Validate the executable inside the installer, not a loose file next to it.
    with tempfile.TemporaryDirectory(prefix="bit-typing-payload-check-") as directory:
        app = Path(directory) / "bit-typing.exe"
        app.write_bytes(reader.extract(name))
        verify_executable(app, expected)


def verify_app(app: Path, expected: dict[str, str]) -> None:
    resources = app / "Contents" / "Resources"
    _check_manifest((resources / MANIFEST).read_bytes(), expected)
    for name, digest in expected.items():
        path = resources / name
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise ValueError(f"macOS app is missing or contains incorrect default file: {name}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("check-source", "verify-executable", "verify-installer", "verify-app"))
    parser.add_argument("artifact", nargs="?", type=Path)
    parser.add_argument("--project", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    if args.mode != "check-source" and args.artifact is None:
        parser.error("artifact path required")
    try:
        expected = inventory(args.project)
        if args.mode == "verify-executable":
            verify_executable(args.artifact, expected)
        elif args.mode == "verify-installer":
            verify_installer(args.artifact, expected)
        elif args.mode == "verify-app":
            verify_app(args.artifact, expected)
    except (OSError, ValueError) as error:
        parser.exit(1, f"Resource check failed: {error}\n")
    print(f"Verified {len(expected)} default resource files ({args.mode}).")


if __name__ == "__main__":
    main()
