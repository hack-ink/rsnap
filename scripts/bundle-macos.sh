#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
	cat <<'EOF'
Usage: scripts/bundle-macos.sh

Bundles Rsnap.app via cargo-bundle, post-processes its Dock icon via Xcode's asset catalog toolchain, then force-launches a new instance unless running in CI.
EOF
	exit 0
fi

if [[ $# -ne 0 ]]; then
	echo "error: this script takes no arguments" >&2
	echo "hint: run: scripts/bundle-macos.sh" >&2
	exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
	echo "error: macOS only (uname=$(uname -s))" >&2
	exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
	echo "error: missing 'cargo' in PATH" >&2
	exit 1
fi

if ! cargo bundle --version >/dev/null 2>&1; then
	echo "error: missing 'cargo-bundle' (try: cargo install cargo-bundle)" >&2
	exit 1
fi

if ! command -v xcrun >/dev/null 2>&1; then
	echo "error: missing 'xcrun' in PATH" >&2
	exit 1
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

icon_composer_asset="${repo_root}/apps/rsnap/assets/app-icon/composer/AppIcon.icon"
if [[ ! -d "${icon_composer_asset}" ]]; then
	echo "error: missing Icon Composer asset at ${icon_composer_asset}" >&2
	exit 1
fi

cargo_profile="${RSNAP_BUNDLE_PROFILE:-release}"
target_triple="${RSNAP_BUNDLE_TARGET:-}"
open_after_bundle="${RSNAP_BUNDLE_OPEN_APP:-1}"

bundle_cmd=(cargo bundle -p rsnap --format osx)
if [[ "${cargo_profile}" == "release" ]]; then
	target_hint="release"
	bundle_cmd+=(--release)
else
	target_hint="${cargo_profile}"
	bundle_cmd+=(--profile "${cargo_profile}")
fi

if [[ -n "${target_triple}" ]]; then
	bundle_cmd+=(--target "${target_triple}")
	target_root="${repo_root}/target/${target_triple}"
else
	target_root="${repo_root}/target"
fi

"${bundle_cmd[@]}"

expected_app="${target_root}/${target_hint}/bundle/osx/Rsnap.app"
app_path=""
if [[ -d "${expected_app}" ]]; then
	app_path="${expected_app}"
else
	app_path="$(find "${target_root}/${target_hint}/bundle/osx" -maxdepth 1 -type d -name "*.app" -print -quit 2>/dev/null || true)"
fi

if [[ -z "${app_path}" || ! -d "${app_path}" ]]; then
	echo "error: failed to locate bundled rsnap.app under target/ (hint=${target_hint})" >&2
	exit 1
fi

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/rsnap-bundle-assets.XXXXXX")"
trap 'rm -rf "${tmp_root}"' EXIT

compiled_assets="${tmp_root}/compiled-assets"
partial_plist="${tmp_root}/asset-info.plist"
mkdir -p "${compiled_assets}"

xcrun actool \
	--compile "${compiled_assets}" \
	--platform macosx \
	--minimum-deployment-target 14.0 \
	--app-icon AppIcon \
	--output-partial-info-plist "${partial_plist}" \
	"${icon_composer_asset}" >/dev/null

if [[ ! -f "${compiled_assets}/Assets.car" || ! -f "${compiled_assets}/AppIcon.icns" ]]; then
	echo "error: actool did not emit the expected app icon assets" >&2
	exit 1
fi

resources_dir="${app_path}/Contents/Resources"
info_plist="${app_path}/Contents/Info.plist"

cp "${compiled_assets}/Assets.car" "${resources_dir}/Assets.car"
cp "${compiled_assets}/AppIcon.icns" "${resources_dir}/AppIcon.icns"

icon_file="$("/usr/libexec/PlistBuddy" -c 'Print :CFBundleIconFile' "${partial_plist}")"
icon_name="$("/usr/libexec/PlistBuddy" -c 'Print :CFBundleIconName' "${partial_plist}")"

"/usr/libexec/PlistBuddy" -c 'Delete :CFBundleIconFile' "${info_plist}" >/dev/null 2>&1 || true
"/usr/libexec/PlistBuddy" -c 'Delete :CFBundleIconName' "${info_plist}" >/dev/null 2>&1 || true
"/usr/libexec/PlistBuddy" -c "Add :CFBundleIconFile string ${icon_file}" "${info_plist}"
"/usr/libexec/PlistBuddy" -c "Add :CFBundleIconName string ${icon_name}" "${info_plist}"

echo "bundled: ${app_path}"
if [[ "${CI:-}" != "true" && "${open_after_bundle}" != "0" ]]; then
	open -n "${app_path}"
fi
