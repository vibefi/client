#!/usr/bin/env bash
# Downloads platform-matched Bun binaries for bundling into the macOS app.
# Usage: ./vendor/fetch-bun.sh [version]
# Outputs: vendor/bun/bun-aarch64-apple-darwin
#          vendor/bun/bun-x86_64-apple-darwin

set -euo pipefail

VERSION="${1:-1.2.4}"
VENDOR_DIR="$(cd "$(dirname "$0")/bun" && pwd)"

fetch_bun() {
    local triple="$1"
    local bun_triple

    case "$triple" in
        aarch64-apple-darwin) bun_triple="bun-darwin-aarch64" ;;
        x86_64-apple-darwin)  bun_triple="bun-darwin-x64" ;;
        *) echo "unsupported triple: $triple" >&2; return 1 ;;
    esac

    local url="https://github.com/oven-sh/bun/releases/download/bun-v${VERSION}/${bun_triple}.zip"
    local tmp
    tmp="$(mktemp -d)"

    echo "Downloading bun ${VERSION} for ${triple}..."
    curl -fsSL "$url" -o "${tmp}/bun.zip"
    unzip -q -o "${tmp}/bun.zip" -d "${tmp}"
    cp "${tmp}/${bun_triple}/bun" "${VENDOR_DIR}/bun-${triple}"
    chmod +x "${VENDOR_DIR}/bun-${triple}"
    rm -rf "$tmp"
    echo "  -> ${VENDOR_DIR}/bun-${triple}"
}

fetch_bun "aarch64-apple-darwin"
fetch_bun "x86_64-apple-darwin"

echo "Done. Vendored Bun binaries are ready."
