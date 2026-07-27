#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat >&2 <<'USAGE'
usage: scripts/release/sign-macos-app.sh --app APP --identity IDENTITY [options]

Signs Rsnap and its embedded Sparkle components from the inside out.

Required:
  --app PATH                         path to Rsnap.app
  --identity IDENTITY                macOS code-signing identity

Optional:
  --keychain PATH                    keychain that contains the signing identity
  --entitlements PATH                entitlements for the outer app

The identity must be an Apple Development identity or "-" for local ad-hoc
signing. Downloader.xpc keeps the entitlements from its Sparkle signature.
USAGE
}

app=""
identity=""
keychain=""
app_entitlements=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--app)
			app="${2:-}"
			shift 2
			;;
		--identity)
			identity="${2:-}"
			shift 2
			;;
		--keychain)
			keychain="${2:-}"
			shift 2
			;;
		--entitlements)
			app_entitlements="${2:-}"
			shift 2
			;;
		-h|--help)
			usage
			exit 0
			;;
		*)
			echo "error: unknown argument: $1" >&2
			usage
			exit 2
			;;
	esac
done

if [[ -z "$app" || -z "$identity" ]]; then
	echo "error: --app and --identity are required" >&2
	usage
	exit 2
fi

if [[ ! -d "$app" ]]; then
	echo "error: app bundle not found: $app" >&2
	exit 1
fi

if [[ -n "$keychain" && ! -f "$keychain" ]]; then
	echo "error: signing keychain not found: $keychain" >&2
	exit 1
fi

if [[ -n "$app_entitlements" && ! -f "$app_entitlements" ]]; then
	echo "error: entitlements file not found: $app_entitlements" >&2
	exit 1
fi

case "$identity" in
	Apple\ Development:*|-)
		;;
	*)
		echo "error: identity must be Apple Development or - for ad-hoc signing" >&2
		exit 1
		;;
esac

codesign_bin="${CODESIGN_BIN:-/usr/bin/codesign}"
if [[ ! -x "$codesign_bin" ]]; then
	echo "error: codesign executable not found: $codesign_bin" >&2
	exit 1
fi

sparkle_framework="$app/Contents/Frameworks/Sparkle.framework"
versions_dir="$sparkle_framework/Versions"
current_link="$versions_dir/Current"
if [[ ! -d "$sparkle_framework" || ! -d "$versions_dir" || ! -L "$current_link" ]]; then
	echo "error: Sparkle.framework does not contain a Versions/Current layout" >&2
	exit 1
fi

versions_real="$(cd "$versions_dir" && pwd -P)"
current_real="$(cd "$current_link" && pwd -P)"
case "$current_real" in
	"$versions_real"/*)
		;;
	*)
		echo "error: Sparkle.framework Versions/Current resolves outside Versions" >&2
		exit 1
		;;
esac

installer="$current_link/XPCServices/Installer.xpc"
downloader="$current_link/XPCServices/Downloader.xpc"
autoupdate="$current_link/Autoupdate"
updater="$current_link/Updater.app"

for component in "$installer" "$downloader" "$autoupdate" "$updater"; do
	if [[ ! -e "$component" ]]; then
		echo "error: required Sparkle signing component not found: $component" >&2
		exit 1
	fi
done

sign_common=(
	--force
	--options runtime
	--sign "$identity"
	--timestamp=none
)

if [[ -n "$keychain" ]]; then
	sign_common+=(--keychain "$keychain")
fi

sign_component() {
	local path="$1"
	shift
	"$codesign_bin" "${sign_common[@]}" "$@" "$path"
}

sign_component "$installer"
sign_component "$downloader" --preserve-metadata=entitlements
sign_component "$autoupdate"
sign_component "$updater"
sign_component "$sparkle_framework"

if [[ -n "$app_entitlements" ]]; then
	sign_component "$app" --entitlements "$app_entitlements"
else
	sign_component "$app"
fi

"$codesign_bin" --verify --deep --strict --verbose=2 "$app"
