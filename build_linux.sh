#!/usr/bin/env bash
# bit-typing Linux build (Rust): release binary + Debian package.
#
# Mirrors the old Python `build_linux.sh` step for step, with cargo in place
# of the Python venv + PyInstaller:
#   1. check-source (default courses / languages / layouts present),
#   2. `cargo build --release`,
#   3. verify the binary (`--verify-resources`, like `verify-executable`),
#   4. stage a .deb (desktop file, icons, control, postinst).
#
# Usage: ./build_linux.sh [version]   (default 2.0.0, must match Cargo.toml)
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "$SCRIPT_DIR/Cargo.toml" && -f "$SCRIPT_DIR/src/main.rs" && -d "$SCRIPT_DIR/courses" && -d "$SCRIPT_DIR/lang" && -d "$SCRIPT_DIR/keyboards" ]]; then
    PROJECT_DIR="$SCRIPT_DIR"
elif [[ -f "$SCRIPT_DIR/../Cargo.toml" && -f "$SCRIPT_DIR/../src/main.rs" ]]; then
    PROJECT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
else
    echo "Error: put this script in the bit-typing project or its outputs folder." >&2
    exit 1
fi

OUTPUT_DIR="$PROJECT_DIR/outputs"
PACKAGE_VERSION="${1:-2.0.0}"
PACKAGE_NAME="bit-typing"
ICON_SOURCE="$PROJECT_DIR/assets/favico.png"
CARGO_TOML_VERSION="$(grep -m1 '^version' "$PROJECT_DIR/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"

if [[ ! "$PACKAGE_VERSION" =~ ^[0-9][0-9A-Za-z.+:~-]*$ ]]; then
    echo "Error: '$PACKAGE_VERSION' is not a valid Debian package version." >&2
    exit 1
fi

if [[ "$PACKAGE_VERSION" != "$CARGO_TOML_VERSION" ]]; then
    echo "Error: version '$PACKAGE_VERSION' does not match Cargo.toml version '$CARGO_TOML_VERSION'." >&2
    echo "Pass the Cargo.toml version, or bump Cargo.toml first." >&2
    exit 1
fi

if [[ ! -f "$ICON_SOURCE" ]]; then
    echo "Error: '$ICON_SOURCE' is missing." >&2
    echo "Add the canonical bit-typing PNG icon before building." >&2
    exit 1
fi

for command_name in cargo dpkg dpkg-deb; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "Error: required command '$command_name' is not installed." >&2
        echo "On Ubuntu, install the Rust toolchain and packaging tools with:" >&2
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
        echo "  sudo apt install dpkg-dev python3-pil" >&2
        exit 1
    fi
done

echo "Checking default resources..."
if [[ -f "$PROJECT_DIR/build_resources.py" ]]; then
    python3 "$PROJECT_DIR/build_resources.py" check-source --project "$PROJECT_DIR"
fi

echo "Running unit tests..."
cargo test --manifest-path "$PROJECT_DIR/Cargo.toml"

echo "Building bit-typing with Cargo (release)..."
cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" --bin bit-typing

BINARY="$PROJECT_DIR/target/release/bit-typing"
if [[ ! -x "$BINARY" ]]; then
    echo "Error: Cargo did not create '$BINARY'." >&2
    exit 1
fi
"$BINARY" --verify-resources
"$BINARY" --version

ARCHITECTURE="$(dpkg --print-architecture)"
STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bit-typing-deb.XXXXXX")"
PACKAGE_ROOT="$STAGING_DIR/$PACKAGE_NAME"

cleanup() {
    rm -rf -- "$STAGING_DIR"
}
trap cleanup EXIT

mkdir -p \
    "$PACKAGE_ROOT/DEBIAN" \
    "$PACKAGE_ROOT/usr/bin" \
    "$PACKAGE_ROOT/usr/share/applications" \
    "$PACKAGE_ROOT/usr/share/pixmaps"

install -m 0755 "$BINARY" "$PACKAGE_ROOT/usr/bin/bit-typing"

# System-wide default resources. The app seeds the writable per-user dirs
# from these on first launch (and falls back to its embedded copies), so a
# fresh install finds lessons, layouts, languages, and artwork immediately.
install -d "$PACKAGE_ROOT/usr/share/bit-typing/courses" \
    "$PACKAGE_ROOT/usr/share/bit-typing/keyboards" \
    "$PACKAGE_ROOT/usr/share/bit-typing/lang" \
    "$PACKAGE_ROOT/usr/share/bit-typing/assets"
for lesson in "$PROJECT_DIR"/courses/*.txt; do
    install -m 0644 "$lesson" "$PACKAGE_ROOT/usr/share/bit-typing/courses/"
done
for layout in "$PROJECT_DIR"/keyboards/*.json; do
    install -m 0644 "$layout" "$PACKAGE_ROOT/usr/share/bit-typing/keyboards/"
done
for language in "$PROJECT_DIR"/lang/*.json; do
    install -m 0644 "$language" "$PACKAGE_ROOT/usr/share/bit-typing/lang/"
done
for artwork in "$PROJECT_DIR"/assets/*.png; do
    install -m 0644 "$artwork" "$PACKAGE_ROOT/usr/share/bit-typing/assets/"
done

cat > "$PACKAGE_ROOT/usr/share/applications/bit-typing.desktop" <<'DESKTOP'
[Desktop Entry]
Version=1.0
Type=Application
Name=BIT Typing
GenericName=Typing Tutor
Comment=Practice typing and review detailed results
Exec=/usr/bin/bit-typing
TryExec=/usr/bin/bit-typing
Icon=bit-typing
Terminal=false
Categories=Education;Utility;
Keywords=typing;keyboard;practice;tutor;
StartupNotify=true
StartupWMClass=bit-typing
DESKTOP

install -m 0644 "$ICON_SOURCE" "$PACKAGE_ROOT/usr/share/pixmaps/bit-typing.png"
python3 - "$ICON_SOURCE" "$PACKAGE_ROOT/usr/share/icons/hicolor" <<'PY'
import sys
from pathlib import Path

source_path = Path(sys.argv[1])
icon_root = Path(sys.argv[2])
try:
    from PIL import Image, ImageOps
except ImportError:
    # Pillow missing: ship the single pixmap only (desktop file still works).
    print("Warning: Pillow is not installed; skipping hicolor icons.", file=sys.stderr)
    sys.exit(0)
with Image.open(source_path) as source:
    source = source.convert("RGBA")
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        fitted = ImageOps.contain(source, (size, size), method=Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        canvas.alpha_composite(fitted, ((size - fitted.width) // 2, (size - fitted.height) // 2))
        destination = icon_root / f"{size}x{size}" / "apps" / "bit-typing.png"
        destination.parent.mkdir(parents=True, exist_ok=True)
        canvas.save(destination, format="PNG", optimize=True)
PY

INSTALLED_SIZE="$(du -sk "$PACKAGE_ROOT/usr" | cut -f1)"
cat > "$PACKAGE_ROOT/DEBIAN/control" <<CONTROL
Package: $PACKAGE_NAME
Version: $PACKAGE_VERSION
Section: education
Priority: optional
Architecture: $ARCHITECTURE
Maintainer: micr0softstore <kokai.balazs@bit-edu.hu>
Installed-Size: $INSTALLED_SIZE
Depends: libc6, libx11-6, libgl1, libxkbcommon0, libwayland-client0, libasound2t64 | libasound2
Description: Modern typing practice application
 bit-typing provides typing lessons, custom courses, a virtual keyboard,
 localized interfaces, and detailed learning statistics.
CONTROL

cat > "$PACKAGE_ROOT/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
POSTINST
chmod 0755 "$PACKAGE_ROOT/DEBIAN/postinst"

mkdir -p "$OUTPUT_DIR"
OUTPUT_DEB="$OUTPUT_DIR/${PACKAGE_NAME}_${PACKAGE_VERSION}_${ARCHITECTURE}.deb"
echo "Creating Debian package..."
dpkg-deb --build --root-owner-group "$PACKAGE_ROOT" "$OUTPUT_DEB"

echo
echo "Package created successfully:"
echo "  $OUTPUT_DEB"
echo
echo "Install it with:"
echo "  sudo apt install '$OUTPUT_DEB'"
