#!/usr/bin/env bash
# Downloads Bun binaries for bundling into packaged apps.
# Usage: ./vendor/fetch-bun.sh [version]
# Outputs: vendor/bun/bun-aarch64-apple-darwin
#          vendor/bun/bun-x86_64-apple-darwin
#          vendor/bun/bun-x86_64-unknown-linux-gnu

set -euo pipefail

VERSION="${1:-1.3.7}"
VENDOR_DIR="$(cd "$(dirname "$0")/bun" && pwd)"

fetch_bun() {
    local triple="$1"
    local -a bun_triples
    local bun_triple

    case "$triple" in
        aarch64-apple-darwin)      bun_triples=("bun-darwin-aarch64") ;;
        x86_64-apple-darwin)       bun_triples=("bun-darwin-x64-baseline" "bun-darwin-x64") ;;
        x86_64-unknown-linux-gnu)  bun_triples=("bun-linux-x64-baseline" "bun-linux-x64") ;;
        *) echo "unsupported triple: $triple" >&2; return 1 ;;
    esac

    local tmp
    local downloaded=0
    tmp="$(mktemp -d)"

    for bun_triple in "${bun_triples[@]}"; do
        local url="https://github.com/oven-sh/bun/releases/download/bun-v${VERSION}/${bun_triple}.zip"
        echo "Downloading bun ${VERSION} for ${triple} (${bun_triple})..."
        if ! curl -fsSL "$url" -o "${tmp}/bun.zip"; then
            continue
        fi

        unzip -q -o "${tmp}/bun.zip" -d "${tmp}"
        cp "${tmp}/${bun_triple}/bun" "${VENDOR_DIR}/bun-${triple}"
        chmod +x "${VENDOR_DIR}/bun-${triple}"
        downloaded=1
        break
    done

    rm -rf "$tmp"

    if [[ "$downloaded" -ne 1 ]]; then
        echo "failed to download bun ${VERSION} for ${triple}" >&2
        return 1
    fi

    echo "  -> ${VENDOR_DIR}/bun-${triple}"
}

fetch_bun "aarch64-apple-darwin"
fetch_bun "x86_64-apple-darwin"
fetch_bun "x86_64-unknown-linux-gnu"

echo "Done. Vendored Bun binaries are ready."
