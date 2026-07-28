#!/usr/bin/env bash
set -euo pipefail

EXPECTED_APPLE_IDENTITY_SUFFIX="RD3D4LH465"
EXPECTED_APPLE_TEAM_ID="T54QFA7W2S"
SAFE_VERSION_NAME_RE='^[A-Za-z0-9][A-Za-z0-9._-]*$'

usage() {
	cat >&2 <<'USAGE'
usage: scripts/release/sign-macos-app.sh --app APP --identity ID [options]

Signs the known Rsnap and Sparkle code graph from the inside out.

Options:
  --keychain PATH     keychain that contains the signing identity
  --entitlements PATH entitlements for the outer app
  --mode MODE         release or development (default: release)
USAGE
}

app_path=""
identity=""
keychain_path=""
app_entitlements=""
mode="release"

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
		--entitlements)
			app_entitlements="${2:-}"
			shift 2
			;;
		--mode)
			mode="${2:-}"
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
if [[ -n "$app_entitlements" && ! -f "$app_entitlements" ]]; then
	echo "error: app entitlements do not exist: $app_entitlements" >&2
	exit 1
fi
if [[ "$mode" == "release" ]]; then
	if [[ ! "$identity" =~ ^Apple\ Development:\ .+\ \(${EXPECTED_APPLE_IDENTITY_SUFFIX}\)$ ]]; then
		echo "error: release signing identity must end with $EXPECTED_APPLE_IDENTITY_SUFFIX" >&2
		exit 1
	fi
	if [[ -z "$keychain_path" ]]; then
		echo "error: release signing requires --keychain" >&2
		exit 1
	fi
elif [[ "$identity" != "-" && ! "$identity" =~ ^Apple\ Development: ]]; then
	echo "error: development signing requires Apple Development or -" >&2
	exit 1
fi

codesign_bin="${RSNAP_CODESIGN_BIN:-${CODESIGN_BIN:-/usr/bin/codesign}}"
if [[ ! -x "$codesign_bin" ]]; then
	echo "error: codesign is not executable: $codesign_bin" >&2
	exit 1
fi

framework="$app_path/Contents/Frameworks/Sparkle.framework"
versions="$framework/Versions"
current_link="$versions/Current"
if [[ ! -d "$framework" || ! -d "$versions" || ! -L "$current_link" ]]; then
	echo "error: Sparkle.framework does not contain a Versions/Current layout" >&2
	exit 1
fi

current_target="$(readlink "$current_link")"
if [[ ! "$current_target" =~ $SAFE_VERSION_NAME_RE || "$current_target" == "Current" ]]; then
	echo "error: Sparkle Versions/Current has an unsafe target" >&2
	exit 1
fi
versions_real="$(cd "$versions" && pwd -P)"
current_real="$(cd "$current_link" && pwd -P)"
if [[ "$(dirname "$current_real")" != "$versions_real" || "$(basename "$current_real")" != "$current_target" ]]; then
	echo "error: Sparkle Versions/Current must resolve to a direct Versions child" >&2
	exit 1
fi

version_directory=""
versions_entry_count=0
versions_layout_invalid=0
while IFS= read -r -d '' entry; do
	versions_entry_count=$((versions_entry_count + 1))
	entry_name="$(basename "$entry")"
	if [[ "$entry_name" == "Current" ]]; then
		if [[ ! -L "$entry" ]]; then
			versions_layout_invalid=1
		fi
	elif [[ "$entry_name" == "$current_target" && -d "$entry" && ! -L "$entry" ]]; then
		version_directory="$entry"
	else
		versions_layout_invalid=1
	fi
done < <(find "$versions" -mindepth 1 -maxdepth 1 -print0)
if [[ "$versions_entry_count" != "2" ||
	"$versions_layout_invalid" != "0" ||
	-z "$version_directory" ||
	"$(cd "$version_directory" && pwd -P)" != "$current_real" ]]; then
	echo "error: Sparkle Versions must contain only Current and its version directory" >&2
	exit 1
fi

installer="$current_real/XPCServices/Installer.xpc"
downloader="$current_real/XPCServices/Downloader.xpc"
autoupdate="$current_real/Autoupdate"
updater="$current_real/Updater.app"
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

expected_bundles="$(
	printf '%s\n' \
		"$current_real/Updater.app" \
		"$current_real/XPCServices/Downloader.xpc" \
		"$current_real/XPCServices/Installer.xpc" |
		sort
)"
discovered_bundles="$(
	find "$current_real" -type d \( -name '*.app' -o -name '*.xpc' -o -name '*.framework' \) -print |
		sort
)"
if [[ "$discovered_bundles" != "$expected_bundles" ]]; then
	echo "error: Sparkle framework contains an unknown nested code bundle" >&2
	exit 1
fi

sign_one() {
	local target="$1"
	shift
	local -a command=("$codesign_bin" --force --sign "$identity")
	if [[ -n "$keychain_path" ]]; then
		command+=(--keychain "$keychain_path")
	fi
	command+=(--options runtime --timestamp=none)
	command+=("$@" "$target")
	"${command[@]}"
}

verify_one() {
	"$codesign_bin" --verify --strict --verbose=4 "$1"
}

sign_one "$installer"
verify_one "$installer"
sign_one "$downloader" --preserve-metadata=entitlements
verify_one "$downloader"
sign_one "$autoupdate"
verify_one "$autoupdate"
sign_one "$updater"
verify_one "$updater"
sign_one "$framework"
verify_one "$framework"
if [[ -n "$app_entitlements" ]]; then
	sign_one "$app_path" --entitlements "$app_entitlements"
else
	sign_one "$app_path"
fi
verify_one "$app_path"

if [[ "$mode" == "release" ]]; then
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
		if ! grep -Fqx "Authority=$identity" <<<"$details"; then
			echo "error: exact Apple Development authority is missing from $target" >&2
			exit 1
		fi
		if grep -q '^Timestamp=' <<<"$details"; then
			echo "error: Apple Development release signatures must not contain a timestamp" >&2
			exit 1
		fi
		if grep -q '^Signature=adhoc' <<<"$details"; then
			echo "error: ad hoc signature found on $target" >&2
			exit 1
		fi
		team="$(sed -n 's/^TeamIdentifier=//p' <<<"$details" | head -n 1)"
		if [[ "$team" != "$EXPECTED_APPLE_TEAM_ID" ]]; then
			echo "error: unexpected Apple TeamIdentifier on $target" >&2
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

# --deep is verification-only. Every signing operation above names one known code object.
"$codesign_bin" --verify --deep --strict --verbose=4 "$app_path"
