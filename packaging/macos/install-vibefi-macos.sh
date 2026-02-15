#!/bin/sh
set -eu

REPO="vibefi/client-staging-public"
LATEST_RELEASE_API="https://api.github.com/repos/${REPO}/releases/latest"
INSTALL_DIR="${HOME}/Applications"
APP_NAME="VibeFi.app"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "This installer is for macOS only." >&2
  exit 1
fi

for cmd in curl shasum tar awk uname mktemp; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 1
  fi
done

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required to parse GitHub release metadata." >&2
  exit 1
fi

arch="$(uname -m)"
case "$arch" in
  arm64|aarch64)
    asset_suffix="_aarch64.app.tar.gz"
    ;;
  x86_64|amd64)
    asset_suffix="_x64.app.tar.gz"
    ;;
  *)
    echo "Unsupported architecture: ${arch}" >&2
    exit 1
    ;;
esac

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

echo "Fetching latest release metadata..."
release_json="$(curl -fsSL -H 'Accept: application/vnd.github+json' "$LATEST_RELEASE_API")"

asset_info="$(
  printf '%s' "$release_json" | python3 -c '
import json
import sys

suffix = sys.argv[1]
release = json.load(sys.stdin)

for asset in release.get("assets", []):
    name = asset.get("name", "")
    if not name.endswith(suffix):
        continue
    digest = asset.get("digest") or ""
    sha = digest.split(":", 1)[1] if digest.lower().startswith("sha256:") else ""
    print(asset.get("browser_download_url", ""))
    print(sha)
    print(name)
    sys.exit(0)

sys.exit(1)
  ' "$asset_suffix"
)" || {
  echo "Could not find a matching macOS artifact (${asset_suffix}) in the latest release." >&2
  exit 1
}

download_url="$(printf '%s\n' "$asset_info" | sed -n '1p')"
expected_sha256="$(printf '%s\n' "$asset_info" | sed -n '2p')"
archive_name="$(printf '%s\n' "$asset_info" | sed -n '3p')"

if [ -z "$download_url" ] || [ -z "$expected_sha256" ] || [ -z "$archive_name" ]; then
  echo "Release metadata was incomplete." >&2
  exit 1
fi

archive_path="${tmp_dir}/${archive_name}"

echo "Downloading ${archive_name}..."
curl -fL --retry 3 --retry-delay 1 --retry-connrefused "$download_url" -o "$archive_path"

echo "Verifying SHA256..."
actual_sha256="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "SHA256 mismatch." >&2
  echo "Expected: $expected_sha256" >&2
  echo "Actual:   $actual_sha256" >&2
  exit 1
fi

echo "Extracting ${archive_name}..."
tar -xzf "$archive_path" -C "$tmp_dir"

app_src="$(find "$tmp_dir" -type d -name "$APP_NAME" -print -quit)"
if [ -z "$app_src" ]; then
  echo "Could not find ${APP_NAME} in the downloaded archive." >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
target_app="${INSTALL_DIR}/${APP_NAME}"

echo "Installing to ${target_app}..."
rm -rf "$target_app"
mv "$app_src" "$target_app"

if command -v xattr >/dev/null 2>&1; then
  # Clear Gatekeeper-related metadata from the installed app bundle.
  xattr -dr com.apple.quarantine "$target_app" 2>/dev/null || true
  xattr -dr com.apple.provenance "$target_app" 2>/dev/null || true
fi

echo "Installed ${APP_NAME} to ${INSTALL_DIR}"
echo "You can launch it from Applications."
