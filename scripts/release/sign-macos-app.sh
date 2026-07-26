#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat >&2 <<'USAGE'
usage: scripts/release/sign-macos-app.sh --app APP --identity ID [options]

Signs the known Rsnap and Sparkle code graph from the inside out.

Options:
  --keychain PATH       keychain that contains the signing identity
  --mode MODE           release or development (default: release)
  --entitlements PATH   outer-app entitlements; development mode only
USAGE
}

app_path=""
identity=""
keychain_path=""
mode="release"
entitlements_path=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--app)
			app_path="${2:-}"
			shift 2
			;;
		--identity)
			identity="${2:-}"
			shift 2
			;;
		--keychain)
			keychain_path="${2:-}"
			shift 2
			;;
		--mode)
			mode="${2:-}"
			shift 2
			;;
		--entitlements)
			entitlements_path="${2:-}"
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

if [[ -z "$app_path" || -z "$identity" ]]; then
	echo "error: --app and --identity are required" >&2
	usage
	exit 2
fi
if [[ "$mode" != "release" && "$mode" != "development" ]]; then
	echo "error: --mode must be release or development" >&2
	exit 2
fi
if [[ ! -d "$app_path" ]]; then
	echo "error: app bundle does not exist: $app_path" >&2
	exit 1
fi
if [[ "$identity" == *$'\n'* || "$identity" == *$'\r'* ]]; then
	echo "error: signing identity must be one line" >&2
	exit 1
fi
if [[ -n "$keychain_path" && ! -f "$keychain_path" ]]; then
	echo "error: signing keychain does not exist: $keychain_path" >&2
	exit 1
fi
if [[ -n "$entitlements_path" && ! -f "$entitlements_path" ]]; then
	echo "error: entitlements file does not exist: $entitlements_path" >&2
	exit 1
fi
if [[ "$mode" == "release" ]]; then
	if [[ "$identity" != "Developer ID Application: "* ]]; then
		echo "error: release signing requires a Developer ID Application identity" >&2
		exit 1
	fi
	if [[ -z "$keychain_path" ]]; then
		echo "error: release signing requires --keychain" >&2
		exit 1
	fi
	if [[ -n "$entitlements_path" ]]; then
		echo "error: release entitlements require a checked-in contract; none is approved" >&2
		exit 1
	fi
fi

codesign_bin="${RSNAP_CODESIGN_BIN:-/usr/bin/codesign}"
if [[ ! -x "$codesign_bin" ]]; then
	echo "error: codesign is not executable: $codesign_bin" >&2
	exit 1
fi

framework="$app_path/Contents/Frameworks/Sparkle.framework"
current_link="$framework/Versions/Current"
if [[ ! -L "$current_link" ]]; then
	echo "error: Sparkle Versions/Current must be a symbolic link" >&2
	exit 1
fi
current_version="$(readlink "$current_link")"
if [[ "$current_version" != "B" ]]; then
	echo "error: unsupported Sparkle framework layout: Versions/Current -> $current_version" >&2
	exit 1
fi
version_root="$framework/Versions/$current_version"
installer="$version_root/XPCServices/Installer.xpc"
downloader="$version_root/XPCServices/Downloader.xpc"
autoupdate="$version_root/Autoupdate"
updater="$version_root/Updater.app"

for required_path in \
	"$installer" \
	"$downloader" \
	"$autoupdate" \
	"$updater" \
	"$framework" \
	"$app_path"; do
	if [[ ! -e "$required_path" ]]; then
		echo "error: required nested code is missing: $required_path" >&2
		exit 1
	fi
done
if [[ ! -f "$autoupdate" || ! -x "$autoupdate" ]]; then
	echo "error: Sparkle Autoupdate must be an executable file" >&2
	exit 1
fi

sign_one() {
	local target="$1"
	shift
	local -a command=("$codesign_bin" --force --sign "$identity")
	if [[ -n "$keychain_path" ]]; then
		command+=(--keychain "$keychain_path")
	fi
	command+=(--options runtime)
	if [[ "$mode" == "release" ]]; then
		command+=(--timestamp)
	else
		command+=(--timestamp=none)
	fi
	command+=("$@" "$target")
	"${command[@]}"
}

verify_one() {
	local target="$1"
	"$codesign_bin" --verify --strict --verbose=4 "$target"
}

sign_one "$installer"
verify_one "$installer"

# Sparkle 2.6 and newer require the Downloader XPC entitlement to survive re-signing.
sign_one "$downloader" --preserve-metadata=entitlements
verify_one "$downloader"

sign_one "$autoupdate"
verify_one "$autoupdate"

sign_one "$updater"
verify_one "$updater"

sign_one "$framework"
verify_one "$framework"

outer_args=()
if [[ -n "$entitlements_path" ]]; then
	outer_args+=(--entitlements "$entitlements_path")
fi
sign_one "$app_path" "${outer_args[@]}"
verify_one "$app_path"

if [[ "$mode" == "release" ]]; then
	expected_team=""
	for target in \
		"$installer" \
		"$downloader" \
		"$autoupdate" \
		"$updater" \
		"$framework" \
		"$app_path"; do
		details="$("$codesign_bin" -dv --verbose=4 "$target" 2>&1)"
		if ! grep -Eq '^CodeDirectory .*flags=.*\(.*runtime.*\)' <<<"$details"; then
			echo "error: hardened runtime is missing from $target" >&2
			exit 1
		fi
		if ! grep -q '^Authority=Developer ID Application:' <<<"$details"; then
			echo "error: Developer ID Application authority is missing from $target" >&2
			exit 1
		fi
		if ! grep -q '^Timestamp=' <<<"$details"; then
			echo "error: secure timestamp is missing from $target" >&2
			exit 1
		fi
		if grep -q '^Signature=adhoc' <<<"$details"; then
			echo "error: ad hoc signature found on $target" >&2
			exit 1
		fi
		team="$(sed -n 's/^TeamIdentifier=//p' <<<"$details" | head -n 1)"
		if [[ -z "$team" ]]; then
			echo "error: TeamIdentifier is missing from $target" >&2
			exit 1
		fi
		if [[ -z "$expected_team" ]]; then
			expected_team="$team"
		elif [[ "$team" != "$expected_team" ]]; then
			echo "error: TeamIdentifier mismatch on $target" >&2
			exit 1
		fi
	done

	outer_entitlements="$("$codesign_bin" -d --entitlements :- "$app_path" 2>/dev/null || true)"
	if grep -Eq 'com\.apple\.security\.(get-task-allow|cs\.disable-library-validation)' \
		<<<"$outer_entitlements"; then
		echo "error: forbidden release entitlement found on $app_path" >&2
		exit 1
	fi
fi

# --deep is verification-only. Signing above always names each code object.
"$codesign_bin" --verify --deep --strict --verbose=4 "$app_path"
