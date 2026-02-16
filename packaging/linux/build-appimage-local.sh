#!/bin/bash
# build-appimage-local.sh — Build a portable AppImage locally using Docker
#
# Replicates the CI pipeline in an Ubuntu 22.04 container so you can test
# the full build + patch flow without pushing to CI.
#
# Uses named Docker volumes to cache apt packages, Rust/cargo, Bun, and
# cargo-packager across runs. First run is slow; subsequent runs skip
# already-installed tools.
#
# Usage:
#   ./packaging/linux/build-appimage-local.sh [output-dir]
#
# To clear caches:
#   docker volume rm vibefi-apt-cache vibefi-apt-lib vibefi-cargo vibefi-rustup vibefi-bun
#
# The output .AppImage will be placed in output-dir (default: ./target/packager/)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUTPUT_DIR="${1:-$PROJECT_ROOT/target/packager}"

mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(realpath "$OUTPUT_DIR")"

echo "==> Building AppImage in Ubuntu 22.04 Docker container..."
echo "    Project: $PROJECT_ROOT"
echo "    Output:  $OUTPUT_DIR"
echo ""

docker run --rm \
    -v "$PROJECT_ROOT":/src \
    -v "$OUTPUT_DIR":/output \
    -v vibefi-apt-cache:/var/cache/apt \
    -v vibefi-apt-lib:/var/lib/apt \
    -v vibefi-cargo:/root/.cargo \
    -v vibefi-rustup:/root/.rustup \
    -v vibefi-bun:/root/.bun \
    -e VIBEFI_EMBED_WC_PROJECT_ID="${VIBEFI_EMBED_WC_PROJECT_ID:-}" \
    --workdir /src \
    ubuntu:22.04 \
    bash -c '
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

TOTAL_START=$SECONDS

echo "==> [1/8] Installing system dependencies..."
SECONDS=0
if dpkg -s libwebkit2gtk-4.1-dev &>/dev/null; then
    echo "    (cached)"
else
    apt-get update -qq
    apt-get install -y -qq \
        curl unzip build-essential pkg-config patchelf \
        libgtk-3-dev \
        libwebkit2gtk-4.1-dev \
        libayatana-appindicator3-dev \
        file
fi
echo "    Done (${SECONDS}s)"

echo ""
echo "==> [2/8] Installing Rust..."
SECONDS=0
if [ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env" && command -v rustc &>/dev/null; then
    echo "    (cached)"
else
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    source "$HOME/.cargo/env"
fi
rustc --version
cargo --version
echo "    Done (${SECONDS}s)"

echo ""
echo "==> [3/8] Installing Bun..."
SECONDS=0
export PATH="$HOME/.bun/bin:$PATH"
if command -v bun &>/dev/null; then
    echo "    (cached)"
else
    curl -fsSL https://bun.sh/install | bash
fi
bun --version
echo "    Done (${SECONDS}s)"

echo ""
echo "==> [4/8] Installing cargo-packager..."
SECONDS=0
if command -v cargo-packager &>/dev/null && cargo packager --version 2>/dev/null | grep -q "0.11.8"; then
    echo "    (cached)"
else
    echo "    Compiling from source, this takes a while..."
    cargo install cargo-packager --version 0.11.8 --locked
fi
cargo packager --version
echo "    Done (${SECONDS}s)"

echo ""
echo "==> [5/8] Fetching Bun binaries for packaging..."
SECONDS=0
./vendor/fetch-bun.sh
echo "    Done (${SECONDS}s)"

echo ""
echo "==> [6/8] Building release binary..."
SECONDS=0
cargo build --release 2>&1
echo "    Done (${SECONDS}s)"

echo ""
echo "==> [7/8] Packaging as AppImage..."
SECONDS=0

# linuxdeploy-plugin-appimage is itself an AppImage — it needs this env var
# to self-extract instead of trying to use FUSE (unavailable in Docker)
export APPIMAGE_EXTRACT_AND_RUN=1

cargo packager --release --formats appimage 2>&1
echo ""
echo "    Packager output:"
ls -lh target/packager/ 2>/dev/null || echo "    (no files in target/packager/)"
echo "    Done (${SECONDS}s)"

echo ""
echo "==> [8/8] Patching AppImage for portability..."
SECONDS=0
APPIMAGE=$(find target/packager -name "*.AppImage" -type f | head -1)
if [ -z "$APPIMAGE" ]; then
    echo "Error: No AppImage found in target/packager/"
    ls -lRh target/packager/ 2>/dev/null
    exit 1
fi
echo "    Found: $APPIMAGE"

export APPIMAGE_EXTRACT_AND_RUN=1
bash packaging/linux/patch-appimage.sh "$APPIMAGE"
echo "    Done (${SECONDS}s)"

echo ""
echo "==> Copying output..."
# Skip copy if source and destination are the same mount
cp -v --no-clobber target/packager/*.AppImage /output/ 2>/dev/null \
    || echo "    (output already in place)"

ELAPSED=$(( SECONDS - TOTAL_START + SECONDS ))
echo ""
echo "========================================="
echo "  Build complete! (total: ${TOTAL_START}s)"
echo "========================================="
'

echo ""
echo "==> AppImage available at: $OUTPUT_DIR/"
ls -lh "$OUTPUT_DIR"/*.AppImage 2>/dev/null || echo "  (no .AppImage files found)"
