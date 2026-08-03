#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PACKAGE_SCRIPT="$ROOT_DIR/scripts/release/package-macos.sh"
ARCHIVE_NAME="rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME="appcast.xml"
CHECKSUM_NAME="$ARCHIVE_NAME.sha256"
FIXTURE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/rsnap-package-test.XXXXXX")"
TOOLS_DIR="$FIXTURE_ROOT/tools"
RUNNER_TEMP="$FIXTURE_ROOT/runner-temp"
LOG_PATH="$FIXTURE_ROOT/tools.log"
mkdir -p "$TOOLS_DIR" "$RUNNER_TEMP"
trap 'rm -rf "$FIXTURE_ROOT"' EXIT

make_tool() {
	local name="$1"
	shift
	printf '%s\n' '#!/usr/bin/env bash' 'set -euo pipefail' "$@" >"$TOOLS_DIR/$name"
	chmod +x "$TOOLS_DIR/$name"
}

make_tool uname 'printf "%s\n" arm64'
make_tool build '
printf "%s\n" build >>"$RELEASE_TOOL_LOG"
for secret in APPLE_CERTIFICATE_P12_BASE64 APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY RSNAP_SPARKLE_PRIVATE_ED_KEY; do
	if [[ -n "${!secret+x}" ]]; then
		echo "error: credential leaked to build: $secret" >&2
		exit 1
	fi
done
if compgen -G "$RUNNER_TEMP/rsnap-release.*/apple-development.p12" >/dev/null; then
	echo "error: certificate materialized before build" >&2
	exit 1
fi
mkdir -p "$RSNAP_NATIVE_HOST_STAGE_DIR/Rsnap.app/Contents"
printf "%s\n" plist >"$RSNAP_NATIVE_HOST_STAGE_DIR/Rsnap.app/Contents/Info.plist"
'
make_tool validator '
printf "%s\n" validator >>"$RELEASE_TOOL_LOG"
count="$(grep -c "^validator$" "$RELEASE_TOOL_LOG")"
if [[ "${FAKE_FAIL_FINAL_MOVE:-0}" == "1" && "$count" == "2" ]]; then
	mkdir -p "$RSNAP_RELEASE_OUTPUT_DIR/$APPCAST_NAME/$APPCAST_NAME"
fi
'
make_tool plutil '
printf "%s\n" plutil >>"$RELEASE_TOOL_LOG"
printf "%s\n" "$FAKE_PUBLIC_KEY"
'
make_tool key-verifier '
printf "%s\n" key-verifier >>"$RELEASE_TOOL_LOG"
cat >/dev/null
'
make_tool base64 '
printf "%s\n" base64 >>"$RELEASE_TOOL_LOG"
cat
'
make_tool openssl '
printf "%s\n" openssl >>"$RELEASE_TOOL_LOG"
printf "%s\n" fixture-keychain-password
'
make_tool security '
command_name="${1:-}"
printf "security:%s\n" "$command_name" >>"$RELEASE_TOOL_LOG"
case "$command_name" in
	create-keychain)
		: >"${@: -1}"
		;;
	delete-keychain)
		rm -f "${@: -1}"
		;;
	find-identity)
		count="${FAKE_IDENTITY_COUNT:-1}"
		printf "%s\n" "  1) AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA \"$APPLE_IDENTITY_FIXTURE\""
		if [[ "$count" == "2" ]]; then
			printf "%s\n" "  2) BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB \"Apple Development: Other (RD3D4LH465)\""
		fi
		printf "     %s valid identities found\n" "$count"
		;;
	esac
'
make_tool sign '
printf "%s\n" sign >>"$RELEASE_TOOL_LOG"
'
make_tool codesign '
printf "%s\n" codesign >>"$RELEASE_TOOL_LOG"
'
make_tool ditto '
printf "%s\n" ditto >>"$RELEASE_TOOL_LOG"
printf "%s\n" zip-bytes >"${@: -1}"
'
make_tool appcast '
printf "%s\n" appcast >>"$RELEASE_TOOL_LOG"
while (($#)); do
	if [[ "$1" == "--appcast" ]]; then
		printf "%s\n" "<rss/>" >"$2"
		exit 0
	fi
	shift
done
exit 1
'
make_tool shasum '
printf "%s\n" shasum >>"$RELEASE_TOOL_LOG"
printf "%064d  %s\n" 0 "${@: -1}"
'

PUBLIC_KEY="$(tr -d '\r\n' <"$ROOT_DIR/scripts/release/sparkle-public-ed-key.txt")"
APPLE_IDENTITY_FIXTURE="Apple Development: Rsnap Release (RD3D4LH465)"
export \
	APPCAST_NAME \
	APPLE_IDENTITY_FIXTURE \
	FAKE_PUBLIC_KEY="$PUBLIC_KEY" \
	RELEASE_TOOL_LOG="$LOG_PATH" \
	RUNNER_ARCH=ARM64 \
	RUNNER_TEMP \
	RSNAP_APPCAST_BIN="$TOOLS_DIR/appcast" \
	RSNAP_ARTIFACT_VALIDATOR_BIN="$TOOLS_DIR/validator" \
	RSNAP_BASE64_BIN="$TOOLS_DIR/base64" \
	RSNAP_BUILD_AND_RUN_BIN="$TOOLS_DIR/build" \
	RSNAP_CODESIGN_BIN="$TOOLS_DIR/codesign" \
	RSNAP_DITTO_BIN="$TOOLS_DIR/ditto" \
	RSNAP_OPENSSL_BIN="$TOOLS_DIR/openssl" \
	RSNAP_PLUTIL_BIN="$TOOLS_DIR/plutil" \
	RSNAP_SECURITY_BIN="$TOOLS_DIR/security" \
	RSNAP_SHASUM_BIN="$TOOLS_DIR/shasum" \
	RSNAP_SIGN_APP_BIN="$TOOLS_DIR/sign" \
	RSNAP_UNAME_BIN="$TOOLS_DIR/uname" \
	RSNAP_VERIFY_SPARKLE_KEY_BIN="$TOOLS_DIR/key-verifier"

release_environment() {
	export \
		APPLE_CERTIFICATE_P12_BASE64=Y2VydA== \
		APPLE_CERTIFICATE_PASSWORD=fixture-password \
		APPLE_SIGNING_IDENTITY="$APPLE_IDENTITY_FIXTURE" \
		RSNAP_RELEASE_TAG=v1.2.3 \
		RSNAP_RELEASE_VERSION=1.2.3 \
		RSNAP_SPARKLE_PRIVATE_ED_KEY=fixture-private-key \
		RSNAP_SPARKLE_VERSION=2.9.5
	unset SPARKLE_PRIVATE_ED_KEY
}

release_environment
SUCCESS_OUTPUT="$FIXTURE_ROOT/success"
RSNAP_RELEASE_OUTPUT_DIR="$SUCCESS_OUTPUT" "$PACKAGE_SCRIPT"
for name in "$ARCHIVE_NAME" "$APPCAST_NAME" "$CHECKSUM_NAME"; do
	[[ -f "$SUCCESS_OUTPUT/$name" ]]
done
build_line="$(grep -n '^build$' "$LOG_PATH" | cut -d: -f1)"
base64_line="$(grep -n '^base64$' "$LOG_PATH" | cut -d: -f1)"
sign_line="$(grep -n '^sign$' "$LOG_PATH" | cut -d: -f1)"
ditto_line="$(grep -n '^ditto$' "$LOG_PATH" | cut -d: -f1)"
appcast_line="$(grep -n '^appcast$' "$LOG_PATH" | cut -d: -f1)"
[[ "$build_line" -lt "$base64_line" ]]
[[ "$sign_line" -lt "$ditto_line" && "$ditto_line" -lt "$appcast_line" ]]
list_keychains_line="$(grep -n '^security:list-keychains$' "$LOG_PATH" | cut -d: -f1)"
default_keychain_line="$(grep -n '^security:default-keychain$' "$LOG_PATH" | cut -d: -f1)"
partition_list_line="$(grep -n '^security:set-key-partition-list$' "$LOG_PATH" | cut -d: -f1)"
[[ "$list_keychains_line" -lt "$default_keychain_line" ]]
[[ "$default_keychain_line" -lt "$partition_list_line" ]]

: >"$LOG_PATH"
release_environment
unset APPLE_SIGNING_IDENTITY
if RSNAP_RELEASE_OUTPUT_DIR="$FIXTURE_ROOT/missing-credential" "$PACKAGE_SCRIPT" >/dev/null 2>&1; then
	echo "error: package accepted a missing signing identity" >&2
	exit 1
fi
[[ ! -s "$LOG_PATH" ]]

: >"$LOG_PATH"
release_environment
PREEXISTING_OUTPUT="$FIXTURE_ROOT/preexisting"
mkdir -p "$PREEXISTING_OUTPUT"
printf '%s\n' preserve >"$PREEXISTING_OUTPUT/$ARCHIVE_NAME"
if RSNAP_RELEASE_OUTPUT_DIR="$PREEXISTING_OUTPUT" "$PACKAGE_SCRIPT" >/dev/null 2>&1; then
	echo "error: package overwrote a preexisting release output" >&2
	exit 1
fi
[[ "$(cat "$PREEXISTING_OUTPUT/$ARCHIVE_NAME")" == preserve ]]
! grep -q '^build$' "$LOG_PATH"

: >"$LOG_PATH"
release_environment
if FAKE_IDENTITY_COUNT=2 RSNAP_RELEASE_OUTPUT_DIR="$FIXTURE_ROOT/two-identities" \
	"$PACKAGE_SCRIPT" >/dev/null 2>&1; then
	echo "error: package accepted more than one codesigning identity" >&2
	exit 1
fi
! grep -q '^sign$' "$LOG_PATH"
[[ -z "$(find "$RUNNER_TEMP" -mindepth 1 -print -quit)" ]]

: >"$LOG_PATH"
release_environment
CLEANUP_OUTPUT="$FIXTURE_ROOT/cleanup"
mkdir -p "$CLEANUP_OUTPUT"
printf '%s\n' preserve >"$CLEANUP_OUTPUT/unrelated.txt"
if FAKE_FAIL_FINAL_MOVE=1 RSNAP_RELEASE_OUTPUT_DIR="$CLEANUP_OUTPUT" \
	"$PACKAGE_SCRIPT" >/dev/null 2>&1; then
	echo "error: package unexpectedly completed the cleanup failure fixture" >&2
	exit 1
fi
[[ "$(cat "$CLEANUP_OUTPUT/unrelated.txt")" == preserve ]]
[[ ! -e "$CLEANUP_OUTPUT/$ARCHIVE_NAME" ]]
[[ -d "$CLEANUP_OUTPUT/$APPCAST_NAME" ]]
[[ ! -e "$CLEANUP_OUTPUT/$CHECKSUM_NAME" ]]
[[ -z "$(find "$RUNNER_TEMP" -mindepth 1 -print -quit)" ]]
