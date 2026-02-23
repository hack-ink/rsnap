#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script is for macOS only (uname=$(uname -s))." >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

require() {
  local bin="$1"
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "Missing dependency: $bin" >&2
    exit 1
  fi
}

require cargo
require rustc
require npm

if ! cargo tauri --version >/dev/null 2>&1; then
  echo "Missing dependency: tauri CLI (cargo tauri)" >&2
  echo "Install: cargo install tauri-cli --locked" >&2
  exit 1
fi

TARGET_TRIPLE="${TARGET_TRIPLE:-$(rustc -vV | awk '/^host: / { print $2 }')}"
if [[ -z "${TARGET_TRIPLE}" ]]; then
  echo "Unable to resolve Rust target triple." >&2
  exit 1
fi

echo "==> Building UI (Vite dist)"
if [[ -f "${ROOT}/ui/package-lock.json" ]]; then
  npm --prefix "${ROOT}/ui" ci
else
  npm --prefix "${ROOT}/ui" install
fi
npm --prefix "${ROOT}/ui" run build

echo "==> Staging native overlay sidecar (rsnap-overlay)"
if ! cargo make --version >/dev/null 2>&1; then
  echo "Missing dependency: cargo-make (cargo make)" >&2
  echo "Install: cargo install cargo-make" >&2
  exit 1
fi

(
  cd "${ROOT}"
  TARGET_TRIPLE="${TARGET_TRIPLE}" cargo make stage-overlay-sidecar
)

SIDE_CAR_SRC="${ROOT}/src-tauri/bin/rsnap-overlay-${TARGET_TRIPLE}"
SIDE_CAR_DEST="${ROOT}/src-tauri/bin/rsnap-overlay"

if [[ ! -f "${SIDE_CAR_SRC}" ]]; then
  echo "Expected staged sidecar not found: ${SIDE_CAR_SRC}" >&2
  exit 1
fi

cp "${SIDE_CAR_SRC}" "${SIDE_CAR_DEST}"
chmod +x "${SIDE_CAR_DEST}" || true
echo "Staged sidecar: ${SIDE_CAR_DEST}"

echo "==> Building Tauri app (release)"
TAURI_BUNDLES="${TAURI_BUNDLES:-app}"
(
  cd "${ROOT}/src-tauri"
  cargo tauri build --bundles "${TAURI_BUNDLES}" --no-sign
)

echo "==> Build outputs"
if [[ -d "${ROOT}/target/release/bundle" ]]; then
  find "${ROOT}/target/release/bundle" -maxdepth 4 -name "*.app" -print || true
  find "${ROOT}/target/release/bundle" -maxdepth 4 -name "*.dmg" -print || true
fi
if [[ -d "${ROOT}/src-tauri/target/release/bundle" ]]; then
  find "${ROOT}/src-tauri/target/release/bundle" -maxdepth 4 -name "*.app" -print || true
  find "${ROOT}/src-tauri/target/release/bundle" -maxdepth 4 -name "*.dmg" -print || true
fi
