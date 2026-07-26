#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CANONICAL_REPOSITORY="acg-box/rsnap"
ARCHIVE_NAME="rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME="appcast.xml"
CHECKSUM_NAME="${ARCHIVE_NAME}.sha256"

required_values=(
	RSNAP_RELEASE_VERSION
	RSNAP_RELEASE_TAG
	RSNAP_SPARKLE_VERSION
	APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64
	APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD
	APPLE_DEVELOPER_ID_APPLICATION_IDENTITY
	APPLE_NOTARY_KEY_ID
	APPLE_NOTARY_ISSUER_ID
	APPLE_NOTARY_KEY_P8
	RSNAP_SPARKLE_PRIVATE_ED_KEY
)
for required_value in "${required_values[@]}"; do
	if [[ -z "${!required_value:-}" ]]; then
		echo "error: missing required release value: $required_value" >&2
		exit 1
	fi
done
if [[ -n "${SPARKLE_PRIVATE_ED_KEY:-}" ]]; then
	echo "error: generic SPARKLE_PRIVATE_ED_KEY is forbidden for Rsnap releases" >&2
	exit 1
fi
if [[ "$RSNAP_RELEASE_TAG" != "v$RSNAP_RELEASE_VERSION" ]]; then
	echo "error: release tag and version do not match" >&2
	exit 1
fi
if [[ "$APPLE_DEVELOPER_ID_APPLICATION_IDENTITY" != "Developer ID Application: "* ]]; then
	echo "error: signing identity must be an exact Developer ID Application identity" >&2
	exit 1
fi

# Keep release credentials in non-exported shell variables. Build tools and dependencies must not
# inherit long-lived Apple or Sparkle secrets from the workflow step environment.
developer_id_p12_base64="$APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64"
developer_id_p12_password="$APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD"
developer_id_identity="$APPLE_DEVELOPER_ID_APPLICATION_IDENTITY"
notary_key_id="$APPLE_NOTARY_KEY_ID"
notary_issuer_id="$APPLE_NOTARY_ISSUER_ID"
notary_key_p8="$APPLE_NOTARY_KEY_P8"
sparkle_private_key="$RSNAP_SPARKLE_PRIVATE_ED_KEY"
unset \
	APPLE_DEVELOPER_ID_APPLICATION_IDENTITY \
	APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64 \
	APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD \
	APPLE_NOTARY_ISSUER_ID \
	APPLE_NOTARY_KEY_ID \
	APPLE_NOTARY_KEY_P8 \
	RSNAP_SPARKLE_PRIVATE_ED_KEY

runner_arch="${RUNNER_ARCH:-}"
uname_bin="${RSNAP_UNAME_BIN:-/usr/bin/uname}"
if [[ "$runner_arch" != "ARM64" || "$("$uname_bin" -m)" != "arm64" ]]; then
	echo "error: Rsnap release packaging requires the macos-26 ARM64 runner" >&2
	exit 1
fi

python_bin="${RSNAP_PYTHON_BIN:-$(command -v python3 || true)}"
security_bin="${RSNAP_SECURITY_BIN:-/usr/bin/security}"
xcrun_bin="${RSNAP_XCRUN_BIN:-/usr/bin/xcrun}"
ditto_bin="${RSNAP_DITTO_BIN:-/usr/bin/ditto}"
codesign_bin="${RSNAP_CODESIGN_BIN:-/usr/bin/codesign}"
spctl_bin="${RSNAP_SPCTL_BIN:-/usr/sbin/spctl}"
build_script="${RSNAP_BUILD_AND_RUN_BIN:-$ROOT_DIR/scripts/build_and_run.sh}"
sign_script="${RSNAP_SIGN_APP_BIN:-$ROOT_DIR/scripts/release/sign-macos-app.sh}"
key_verifier="${RSNAP_VERIFY_SPARKLE_KEY_BIN:-$ROOT_DIR/scripts/release/verify-sparkle-key.swift}"
appcast_script="${RSNAP_APPCAST_BIN:-$ROOT_DIR/scripts/release/sparkle-appcast.sh}"
artifact_validator="${RSNAP_ARTIFACT_VALIDATOR_BIN:-$ROOT_DIR/scripts/release/validate-release-artifacts.py}"

for executable in \
	"$python_bin" \
	"$security_bin" \
	"$xcrun_bin" \
	"$ditto_bin" \
	"$codesign_bin" \
	"$spctl_bin" \
	"$build_script" \
	"$sign_script" \
	"$key_verifier" \
	"$appcast_script" \
	"$artifact_validator"; do
	if [[ ! -x "$executable" ]]; then
		echo "error: required release tool is not executable: $executable" >&2
		exit 1
	fi
done

runner_temp="${RUNNER_TEMP:-}"
if [[ -z "$runner_temp" || ! -d "$runner_temp" ]]; then
	echo "error: RUNNER_TEMP must name an existing directory" >&2
	exit 1
fi
output_dir="${RSNAP_RELEASE_OUTPUT_DIR:-$ROOT_DIR/dist}"
mkdir -p "$output_dir"
archive_path="$output_dir/$ARCHIVE_NAME"
appcast_path="$output_dir/$APPCAST_NAME"
checksum_path="$output_dir/$CHECKSUM_NAME"
rm -f "$archive_path" "$appcast_path" "$checksum_path"

umask 077
work_root="$(mktemp -d "$runner_temp/rsnap-release.XXXXXX")"
stage_dir="$work_root/stage"
certificate_path="$work_root/developer-id.p12"
notary_key_path="$work_root/AuthKey_${notary_key_id}.p8"
keychain_path="$work_root/release.keychain-db"
notary_archive="$work_root/Rsnap-notary.zip"
notary_submit_result="$work_root/notary-submit.json"
notary_result="$work_root/notary-result.json"
keychain_created=0
package_complete=0

cleanup() {
	if [[ "$package_complete" != "1" ]]; then
		rm -f "$archive_path" "$appcast_path" "$checksum_path" || true
	fi
	if [[ "$keychain_created" == "1" ]]; then
		"$security_bin" delete-keychain "$keychain_path" >/dev/null 2>&1 || true
	fi
	if [[ "$work_root" == "$runner_temp"/rsnap-release.* && -d "$work_root" ]]; then
		rm -rf "$work_root" || true
	fi
}
trap cleanup EXIT

# Build and validate the unsigned app before any credential is written to disk. The Apple and
# Sparkle secret names were unset above, so build tools cannot inherit them.
RSNAP_NATIVE_HOST_APP_VERSION="$RSNAP_RELEASE_VERSION" \
	RSNAP_NATIVE_HOST_FORCE_REBUILD=1 \
	RSNAP_NATIVE_HOST_RUST_PROFILE=final-release \
	RSNAP_NATIVE_HOST_SKIP_SIGNING=1 \
	RSNAP_NATIVE_HOST_STAGE_DIR="$stage_dir" \
	RSNAP_NATIVE_HOST_SWIFT_CLEAN=1 \
	RSNAP_NATIVE_HOST_SWIFT_CONFIGURATION=release \
	"$build_script" stage

app_path="$stage_dir/Rsnap.app"
"$artifact_validator" \
	--app "$app_path" \
	--repository "$CANONICAL_REPOSITORY" \
	--sparkle-version "$RSNAP_SPARKLE_VERSION" \
	--tag "$RSNAP_RELEASE_TAG" \
	--version "$RSNAP_RELEASE_VERSION"

sparkle_public_key="$("$python_bin" - "$app_path/Contents/Info.plist" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "rb") as handle:
	value = plistlib.load(handle).get("SUPublicEDKey")
if not isinstance(value, str) or not value:
	raise SystemExit("error: staged app is missing SUPublicEDKey")
print(value)
PY
)"
printf '%s\n' "$sparkle_private_key" \
	| "$key_verifier" "$sparkle_public_key"

CERTIFICATE_PATH="$certificate_path" \
	APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64="$developer_id_p12_base64" \
	"$python_bin" - <<'PY'
import base64
import binascii
import os
from pathlib import Path

try:
	data = base64.b64decode(
		os.environ["APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64"],
		validate=True,
	)
except (ValueError, binascii.Error) as error:
	raise SystemExit(f"error: Developer ID certificate is not valid base64: {error}") from error
if not data:
	raise SystemExit("error: decoded Developer ID certificate is empty")
Path(os.environ["CERTIFICATE_PATH"]).write_bytes(data)
PY
printf '%s' "$notary_key_p8" >"$notary_key_path"
if [[ ! -s "$notary_key_path" ]]; then
	echo "error: decoded Apple notary key is empty" >&2
	exit 1
fi

keychain_password="$("$python_bin" -c 'import secrets; print(secrets.token_hex(24))')"
"$security_bin" create-keychain -p "$keychain_password" "$keychain_path"
keychain_created=1
"$security_bin" set-keychain-settings -lut 21600 "$keychain_path"
"$security_bin" unlock-keychain -p "$keychain_password" "$keychain_path"
"$security_bin" import "$certificate_path" \
	-k "$keychain_path" \
	-P "$developer_id_p12_password" \
	-T "$codesign_bin" \
	-T "$security_bin"
"$security_bin" set-key-partition-list \
	-S apple-tool:,apple: \
	-s \
	-k "$keychain_password" \
	"$keychain_path"

identity_list="$("$security_bin" find-identity -v -p codesigning "$keychain_path")"
identity_matches="$(
	grep -F "\"$developer_id_identity\"" <<<"$identity_list" || true
)"
identity_match_count="$(grep -c . <<<"$identity_matches" || true)"
if [[ "$identity_match_count" != "1" ]]; then
	echo "error: release keychain must contain exactly one requested Developer ID identity" >&2
	exit 1
fi

RSNAP_CODESIGN_BIN="$codesign_bin" \
	"$sign_script" \
	--app "$app_path" \
	--identity "$developer_id_identity" \
	--keychain "$keychain_path" \
	--mode release

"$ditto_bin" -c -k --keepParent "$app_path" "$notary_archive"
"$xcrun_bin" notarytool submit "$notary_archive" \
	--output-format json \
	--key "$notary_key_path" \
	--key-id "$notary_key_id" \
	--issuer "$notary_issuer_id" \
	>"$notary_submit_result"

notary_submission_id="$("$python_bin" - "$notary_submit_result" <<'PY'
import json
import re
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
	result = json.load(handle)
submission_id = result.get("id")
if not isinstance(submission_id, str) or re.fullmatch(
	r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
	submission_id,
) is None:
	raise SystemExit("error: notarytool submit result is missing a valid submission id")
print(submission_id)
PY
)"
echo "Apple notarization submission: $notary_submission_id"
if ! "$xcrun_bin" notarytool wait "$notary_submission_id" \
	--timeout 30m \
	--output-format json \
	--key "$notary_key_path" \
	--key-id "$notary_key_id" \
	--issuer "$notary_issuer_id" \
	>"$notary_result"; then
	echo "error: Apple notarization wait failed; submission remains $notary_submission_id" >&2
	exit 1
fi

notary_status="$("$python_bin" - "$notary_result" "$notary_submission_id" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
	result = json.load(handle)
status = result.get("status")
submission_id = result.get("id")
if submission_id != sys.argv[2]:
	raise SystemExit("error: notarytool wait returned a different submission id")
if not isinstance(status, str):
	raise SystemExit("error: notarytool result is missing status")
print(f"{status}\t{submission_id}")
PY
)"
if [[ "${notary_status%%$'\t'*}" != "Accepted" ]]; then
	echo "error: Apple notarization was not accepted: $notary_status" >&2
	exit 1
fi

"$xcrun_bin" stapler staple "$app_path"
"$xcrun_bin" stapler validate -v "$app_path"
"$codesign_bin" --verify --deep --strict --verbose=4 "$app_path"
"$spctl_bin" --assess --type execute --verbose=4 "$app_path"

# The public archive is created only after notarization, stapling, and Gatekeeper checks.
"$ditto_bin" -c -k --sequesterRsrc --keepParent "$app_path" "$archive_path"
if [[ ! -s "$archive_path" ]]; then
	echo "error: final release archive was not created" >&2
	exit 1
fi
RSNAP_SPARKLE_PRIVATE_ED_KEY="$sparkle_private_key" \
	"$appcast_script" \
	--archive "$archive_path" \
	--appcast "$appcast_path" \
	--version "$RSNAP_RELEASE_VERSION" \
	--tag "$RSNAP_RELEASE_TAG"

"$python_bin" - "$archive_path" "$checksum_path" <<'PY'
import hashlib
import sys
from pathlib import Path

archive = Path(sys.argv[1])
checksum = Path(sys.argv[2])
hasher = hashlib.sha256()
with archive.open("rb") as handle:
	for chunk in iter(lambda: handle.read(1 << 20), b""):
		hasher.update(chunk)
digest = hasher.hexdigest()
checksum.write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
PY

"$artifact_validator" \
	--archive "$archive_path" \
	--appcast "$appcast_path" \
	--checksum "$checksum_path" \
	--repository "$CANONICAL_REPOSITORY" \
	--sparkle-version "$RSNAP_SPARKLE_VERSION" \
	--tag "$RSNAP_RELEASE_TAG" \
	--version "$RSNAP_RELEASE_VERSION"

package_complete=1
printf 'Prepared notarized release assets:\n  %s\n  %s\n  %s\n' \
	"$archive_path" \
	"$appcast_path" \
	"$checksum_path"
