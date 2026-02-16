#!/bin/bash
# build-appimage.sh — Build a portable AppImage from the release binary
#
# Bypasses cargo-packager's linuxdeploy (which fails without FUSE) and builds
# the AppImage directly using ldd + appimagetool. Includes WebKit binary
# patching for cross-distro portability.
#
# Usage:
#   ./packaging/linux/build-appimage.sh <release-binary> [output-path]
#
# Example:
#   ./packaging/linux/build-appimage.sh target/release/vibefi target/packager/vibefi.AppImage
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [ $# -lt 1 ]; then
    echo "Usage: $0 <release-binary> [output-path]"
    exit 1
fi

BINARY="$(realpath "$1")"
BINARY_NAME="$(basename "$BINARY")"
OUTPUT_PATH="${2:-$PROJECT_ROOT/target/packager/${BINARY_NAME}_$(cat "$PROJECT_ROOT/Cargo.toml" | grep '^version' | head -1 | sed 's/.*"\(.*\)".*/\1/')_x86_64.AppImage}"

if [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found: $BINARY"
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT_PATH")"
OUTPUT_PATH="$(realpath "$OUTPUT_PATH")"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

APPDIR="$WORK_DIR/vibefi.AppDir"

echo "==> Building AppImage from: $BINARY"
echo "    Output: $OUTPUT_PATH"
echo ""

# --- Create AppDir structure ---
echo "==> [1/6] Creating AppDir structure..."
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/lib"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# Copy binary
cp "$BINARY" "$APPDIR/usr/bin/$BINARY_NAME"
chmod +x "$APPDIR/usr/bin/$BINARY_NAME"

# Copy resources that cargo-packager would normally bundle
RESOURCES_DIR="$APPDIR/usr/lib/$BINARY_NAME"
mkdir -p "$RESOURCES_DIR"
while IFS= read -r res; do
    # Trim whitespace and quotes
    res="$(echo "$res" | sed 's/^[[:space:]]*"//;s/"[[:space:]]*,*$//')"
    [ -z "$res" ] && continue
    [[ "$res" == \#* ]] && continue
    [[ "$res" == \]* ]] && continue

    src="$PROJECT_ROOT/$res"
    if [ -f "$src" ]; then
        cp -v "$src" "$RESOURCES_DIR/"
    else
        echo "    Warning: Resource not found: $src"
    fi
done < <(sed -n '/^resources/,/^\]/p' "$PROJECT_ROOT/Cargo.toml" | tail -n +2)

# Copy external binaries
EXTERNAL_BIN_SRC="$PROJECT_ROOT/vendor/bun/bun-x86_64-unknown-linux-gnu"
if [ -f "$EXTERNAL_BIN_SRC" ]; then
    cp -v "$EXTERNAL_BIN_SRC" "$APPDIR/usr/bin/"
    chmod +x "$APPDIR/usr/bin/bun-x86_64-unknown-linux-gnu"
else
    echo "    Warning: External binary not found: $EXTERNAL_BIN_SRC"
fi

# Create .desktop file
cat > "$APPDIR/usr/share/applications/$BINARY_NAME.desktop" << DESKTOP
[Desktop Entry]
Name=VibeFi
Exec=$BINARY_NAME %u
Icon=$BINARY_NAME
Type=Application
Categories=Utility;
DESKTOP
ln -sf "usr/share/applications/$BINARY_NAME.desktop" "$APPDIR/$BINARY_NAME.desktop"

# Copy icon
ICON_SRC="$PROJECT_ROOT/packaging/icons/vibefi.png"
if [ -f "$ICON_SRC" ]; then
    cp "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/256x256/apps/$BINARY_NAME.png"
    cp "$ICON_SRC" "$APPDIR/.DirIcon"
    ln -sf ".DirIcon" "$APPDIR/$BINARY_NAME.png"
fi

# --- Bundle shared libraries ---
echo ""
echo "==> [2/6] Bundling shared libraries..."
LIB_DIR="$APPDIR/usr/lib"

# List of libraries to NOT bundle (system/driver libs that must come from host)
EXCLUDE_LIBS=(
    "linux-vdso.so"
    "ld-linux"
    "libc.so"
    "libm.so"
    "libdl.so"
    "librt.so"
    "libpthread.so"
    "libresolv.so"
    "libnss_"
    "libGL.so"
    "libEGL.so"
    "libGLX.so"
    "libGLdispatch.so"
    "libvulkan.so"
    "libnvidia"
    "libdrm.so"
    # Must come from host to stay ABI-compatible with host EGL/Wayland stack.
    "libwayland-client.so"
    "libwayland-cursor.so"
    "libwayland-egl.so"
    "libwayland-server.so"
)

should_exclude() {
    local lib="$1"
    for pattern in "${EXCLUDE_LIBS[@]}"; do
        if [[ "$lib" == *"$pattern"* ]]; then
            return 0
        fi
    done
    return 1
}

# Collect all needed libraries recursively
LIBS_TO_BUNDLE=()
SEEN_LIBS=()

collect_libs() {
    local target="$1"
    ldd "$target" 2>/dev/null | while read -r line; do
        # Parse "libfoo.so.1 => /usr/lib/libfoo.so.1 (0x...)" format
        local lib_path
        lib_path="$(echo "$line" | grep -oP '=> \K/[^ ]+' || true)"
        [ -z "$lib_path" ] && continue
        [ ! -f "$lib_path" ] && continue

        local lib_name
        lib_name="$(basename "$lib_path")"

        # Skip if excluded or already seen
        should_exclude "$lib_name" && continue

        # Check if already in our lib dir
        [ -f "$LIB_DIR/$lib_name" ] && continue

        echo "$lib_path"
    done
}

# Iteratively resolve dependencies (up to 5 rounds to catch transitive deps)
for round in 1 2 3 4 5; do
    NEW_LIBS=()
    # Collect from the main binary and all already-bundled libs
    while IFS= read -r lib_path; do
        [ -z "$lib_path" ] && continue
        lib_name="$(basename "$lib_path")"
        if [ ! -f "$LIB_DIR/$lib_name" ]; then
            cp "$lib_path" "$LIB_DIR/$lib_name"
            NEW_LIBS+=("$LIB_DIR/$lib_name")
        fi
    done < <(
        collect_libs "$APPDIR/usr/bin/$BINARY_NAME"
        for bundled in "$LIB_DIR"/*.so*; do
            [ -f "$bundled" ] && collect_libs "$bundled"
        done
    )

    if [ ${#NEW_LIBS[@]} -eq 0 ]; then
        echo "    Resolved all dependencies in $round round(s)"
        break
    fi
    echo "    Round $round: bundled ${#NEW_LIBS[@]} new libraries"
done

LIB_COUNT=$(find "$LIB_DIR" -name '*.so*' -type f | wc -l)
echo "    Total bundled libraries: $LIB_COUNT"

# --- Copy WebKit subprocess executables ---
echo ""
echo "==> [3/6] Copying WebKit subprocess executables..."

WEBKIT_LIB_DIR=""
for candidate in \
    /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 \
    /usr/lib64/webkit2gtk-4.1 \
    /usr/lib/webkit2gtk-4.1; do
    if [ -d "$candidate" ]; then
        WEBKIT_LIB_DIR="$candidate"
        break
    fi
done

if [ -z "$WEBKIT_LIB_DIR" ]; then
    echo "    Error: Could not find WebKit lib directory"
    exit 1
fi

WEBKIT_REL_DIR="${WEBKIT_LIB_DIR#/}"
WEBKIT_DEST="$APPDIR/$WEBKIT_REL_DIR"
mkdir -p "$WEBKIT_DEST"

for exe in WebKitNetworkProcess WebKitWebProcess; do
    if [ -f "$WEBKIT_LIB_DIR/$exe" ]; then
        cp -v "$WEBKIT_LIB_DIR/$exe" "$WEBKIT_DEST/$exe"
    else
        echo "    Warning: $exe not found"
    fi
done

BUNDLE_DIR="$WEBKIT_DEST/injected-bundle"
mkdir -p "$BUNDLE_DIR"
if [ -f "$WEBKIT_LIB_DIR/injected-bundle/libwebkit2gtkinjectedbundle.so" ]; then
    cp -v "$WEBKIT_LIB_DIR/injected-bundle/libwebkit2gtkinjectedbundle.so" \
          "$BUNDLE_DIR/"
fi

# --- Binary-patch WebKit libraries ---
echo ""
echo "==> [4/6] Binary-patching WebKit libraries (/usr -> ././)..."
find "$LIB_DIR" -type f \( -name 'libwebkit2gtk*' -o -name 'libjavascriptcoregtk*' \) | while read -r lib; do
    echo "    Patching: $(basename "$lib")"
    sed -i 's|/usr|././|g' "$lib"
done

# --- Install custom AppRun ---
echo ""
echo "==> [5/6] Installing custom AppRun..."
cp "$SCRIPT_DIR/AppRun" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"

# --- Repack with appimagetool ---
echo ""
echo "==> [6/6] Building AppImage with appimagetool..."

APPIMAGETOOL="$WORK_DIR/appimagetool"
if ! command -v appimagetool &>/dev/null; then
    echo "    Downloading appimagetool..."
    curl -fsSL -o "$APPIMAGETOOL" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL"
else
    APPIMAGETOOL="$(command -v appimagetool)"
fi

# appimagetool is itself an AppImage; needs FUSE or extract-and-run
export APPIMAGE_EXTRACT_AND_RUN=1
ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$OUTPUT_PATH"

echo ""
echo "==> Done! AppImage: $OUTPUT_PATH"
ls -lh "$OUTPUT_PATH"

# Verify
echo ""
echo "==> Verifying..."
PATCHED_COUNT=$(find "$LIB_DIR" -type f -name 'libwebkit2gtk*' -exec strings {} \; | grep -c '././' || true)
if [ "$PATCHED_COUNT" -gt 0 ]; then
    echo "    OK: Found $PATCHED_COUNT '././' references in patched libraries"
else
    echo "    WARNING: No '././' references found — patch may not have applied"
fi
