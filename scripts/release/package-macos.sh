#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CANONICAL_REPOSITORY="acg-box/rsnap"
CANONICAL_FEED_URL="https://github.com/${CANONICAL_REPOSITORY}/releases/latest/download/appcast.xml"
ARCHIVE_NAME="rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME="appcast.xml"
CHECKSUM_NAME="${ARCHIVE_NAME}.sha256"
EXPECTED_APPLE_IDENTITY_SUFFIX="RD3D4LH465"
EXPECTED_SPARKLE_VERSION="2.9.4"
PUBLIC_KEY_FILE="$ROOT_DIR/scripts/release/sparkle-public-ed-key.txt"

required_values=(
	RSNAP_RELEASE_VERSION
	RSNAP_RELEASE_TAG
	RSNAP_SPARKLE_VERSION
	APPLE_CERTIFICATE_P12_BASE64
	APPLE_CERTIFICATE_PASSWORD
	APPLE_SIGNING_IDENTITY
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
if [[ ! "$RSNAP_RELEASE_VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
	echo "error: release version must be stable semantic version" >&2
	exit 1
fi
if [[ "$RSNAP_RELEASE_TAG" != "v$RSNAP_RELEASE_VERSION" ]]; then
	echo "error: release tag and version do not match" >&2
	exit 1
fi
if [[ "$RSNAP_SPARKLE_VERSION" != "$EXPECTED_SPARKLE_VERSION" ]]; then
	echo "error: release Sparkle version must be $EXPECTED_SPARKLE_VERSION" >&2
	exit 1
fi
if [[ ! "$APPLE_SIGNING_IDENTITY" =~ ^Apple\ Development:\ .+\ \(${EXPECTED_APPLE_IDENTITY_SUFFIX}\)$ ]]; then
	echo "error: signing identity must end with $EXPECTED_APPLE_IDENTITY_SUFFIX" >&2
	exit 1
fi

# Keep secrets in non-exported shell variables. Build tools receive only public release metadata.
certificate_p12_base64="$APPLE_CERTIFICATE_P12_BASE64"
certificate_password="$APPLE_CERTIFICATE_PASSWORD"
signing_identity="$APPLE_SIGNING_IDENTITY"
sparkle_private_key="$RSNAP_SPARKLE_PRIVATE_ED_KEY"
release_version="$RSNAP_RELEASE_VERSION"
release_tag="$RSNAP_RELEASE_TAG"
sparkle_version="$RSNAP_SPARKLE_VERSION"
unset \
	APPLE_CERTIFICATE_P12_BASE64 \
	APPLE_CERTIFICATE_PASSWORD \
	APPLE_SIGNING_IDENTITY \
	RSNAP_SPARKLE_PRIVATE_ED_KEY

uname_bin="${RSNAP_UNAME_BIN:-/usr/bin/uname}"
if [[ "${RUNNER_ARCH:-}" != "ARM64" || "$("$uname_bin" -m)" != "arm64" ]]; then
	echo "error: Rsnap release packaging requires the macos-26 ARM64 runner" >&2
	exit 1
fi
if [[ -z "${RUNNER_TEMP:-}" || ! -d "$RUNNER_TEMP" ]]; then
	echo "error: RUNNER_TEMP must name an existing directory" >&2
	exit 1
fi
if [[ ! -f "$PUBLIC_KEY_FILE" ]]; then
	echo "error: Sparkle public key file does not exist" >&2
	exit 1
fi

security_bin="${RSNAP_SECURITY_BIN:-/usr/bin/security}"
codesign_bin="${RSNAP_CODESIGN_BIN:-/usr/bin/codesign}"
ditto_bin="${RSNAP_DITTO_BIN:-/usr/bin/ditto}"
plutil_bin="${RSNAP_PLUTIL_BIN:-/usr/bin/plutil}"
shasum_bin="${RSNAP_SHASUM_BIN:-/usr/bin/shasum}"
base64_bin="${RSNAP_BASE64_BIN:-/usr/bin/base64}"
openssl_bin="${RSNAP_OPENSSL_BIN:-$(command -v openssl || true)}"
build_script="${RSNAP_BUILD_AND_RUN_BIN:-$ROOT_DIR/scripts/build_and_run.sh}"
sign_script="${RSNAP_SIGN_APP_BIN:-$ROOT_DIR/scripts/release/sign-macos-app.sh}"
key_verifier="${RSNAP_VERIFY_SPARKLE_KEY_BIN:-$ROOT_DIR/scripts/release/verify-sparkle-key.swift}"
appcast_script="${RSNAP_APPCAST_BIN:-$ROOT_DIR/scripts/release/sparkle-appcast.py}"
artifact_validator="${RSNAP_ARTIFACT_VALIDATOR_BIN:-$ROOT_DIR/scripts/release/validate-release-artifacts.py}"
for executable in \
	"$security_bin" \
	"$uname_bin" \
	"$codesign_bin" \
	"$ditto_bin" \
	"$plutil_bin" \
	"$shasum_bin" \
	"$base64_bin" \
	"$openssl_bin" \
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

sparkle_public_key="$(tr -d '\r\n' <"$PUBLIC_KEY_FILE")"

output_dir="${RSNAP_RELEASE_OUTPUT_DIR:-$ROOT_DIR/dist}"
mkdir -p "$output_dir"
for output_name in "$ARCHIVE_NAME" "$APPCAST_NAME" "$CHECKSUM_NAME"; do
	if [[ -e "$output_dir/$output_name" || -L "$output_dir/$output_name" ]]; then
		echo "error: release output already exists: $output_dir/$output_name" >&2
		exit 1
	fi
done
umask 077
work_root="$(mktemp -d "$RUNNER_TEMP/rsnap-release.XXXXXX")"
stage_dir="$work_root/stage"
certificate_path="$work_root/apple-development.p12"
keychain_path="$work_root/release.keychain-db"
archive_path="$work_root/$ARCHIVE_NAME"
appcast_path="$work_root/$APPCAST_NAME"
checksum_path="$work_root/$CHECKSUM_NAME"
keychain_created=0
package_complete=0
archive_landed=0
appcast_landed=0
checksum_landed=0

cleanup() {
	if [[ "$keychain_created" == "1" ]]; then
		"$security_bin" delete-keychain "$keychain_path" >/dev/null 2>&1 || true
	fi
	if [[ "$work_root" == "$RUNNER_TEMP"/rsnap-release.* && -d "$work_root" ]]; then
		rm -rf "$work_root" || true
	fi
	if [[ "$package_complete" != "1" ]]; then
		if [[ "$archive_landed" == "1" ]]; then
			rm -f "$output_dir/$ARCHIVE_NAME" || true
		fi
		if [[ "$appcast_landed" == "1" ]]; then
			rm -f "$output_dir/$APPCAST_NAME" || true
		fi
		if [[ "$checksum_landed" == "1" ]]; then
			rm -f "$output_dir/$CHECKSUM_NAME" || true
		fi
	fi
}
trap cleanup EXIT

RSNAP_NATIVE_HOST_FORCE_REBUILD=1 \
	RSNAP_NATIVE_HOST_APP_VERSION="$release_version" \
	RSNAP_NATIVE_HOST_SIGN_IDENTITY="-" \
	RSNAP_NATIVE_HOST_STAGE_DIR="$stage_dir" \
	RSNAP_SPARKLE_APPCAST_URL="$CANONICAL_FEED_URL" \
	RSNAP_SPARKLE_PUBLIC_ED_KEY="$sparkle_public_key" \
	"$build_script" stage

app_path="$stage_dir/Rsnap.app"
"$artifact_validator" \
	--app "$app_path" \
	--repository "$CANONICAL_REPOSITORY" \
	--sparkle-version "$sparkle_version" \
	--tag "$release_tag" \
	--version "$release_version"
if [[ "$("$plutil_bin" -extract SUPublicEDKey raw "$app_path/Contents/Info.plist")" != "$sparkle_public_key" ]]; then
	echo "error: staged Sparkle public key changed during the build" >&2
	exit 1
fi
printf '%s\n' "$sparkle_private_key" | "$key_verifier" "$sparkle_public_key"

# Credentials are written only after the build and unsigned artifact checks succeed.
printf '%s' "$certificate_p12_base64" | "$base64_bin" -D >"$certificate_path"
if [[ ! -s "$certificate_path" ]]; then
	echo "error: decoded Apple Development certificate is empty" >&2
	exit 1
fi
keychain_password="$("$openssl_bin" rand -hex 24)"
"$security_bin" create-keychain -p "$keychain_password" "$keychain_path"
keychain_created=1
"$security_bin" set-keychain-settings -lut 21600 "$keychain_path"
"$security_bin" unlock-keychain -p "$keychain_password" "$keychain_path"
"$security_bin" import "$certificate_path" \
	-k "$keychain_path" \
	-P "$certificate_password" \
	-T "$codesign_bin" \
	-T "$security_bin"
"$security_bin" list-keychains -d user -s "$keychain_path"
"$security_bin" default-keychain -d user -s "$keychain_path"
"$security_bin" set-key-partition-list \
	-S apple-tool:,apple: \
	-s \
	-k "$keychain_password" \
	"$keychain_path"

identity_list="$("$security_bin" find-identity -v -p codesigning "$keychain_path")"
identity_matches="$(grep -F "\"$signing_identity\"" <<<"$identity_list" || true)"
valid_identity_lines="$(
	grep -Ec '^[[:space:]]*[0-9]+\) [0-9A-Fa-f]{40} ".+"$' <<<"$identity_list" || true
)"
valid_identity_footer="$(
	grep -Ec '^[[:space:]]*1 valid identities found$' <<<"$identity_list" || true
)"
if [[ "$valid_identity_lines" != "1" ||
	"$valid_identity_footer" != "1" ||
	"$(grep -c . <<<"$identity_matches" || true)" != "1" ]]; then
	echo "error: release keychain must contain exactly one requested certificate/private-key pair" >&2
	exit 1
fi

RSNAP_CODESIGN_BIN="$codesign_bin" \
	"$sign_script" \
	--app "$app_path" \
	--identity "$signing_identity" \
	--keychain "$keychain_path" \
	--mode release
"$codesign_bin" --verify --deep --strict --verbose=4 "$app_path"

"$ditto_bin" -c -k --sequesterRsrc --keepParent "$app_path" "$archive_path"
if [[ ! -s "$archive_path" ]]; then
	echo "error: final release archive was not created" >&2
	exit 1
fi
RSNAP_SPARKLE_PRIVATE_ED_KEY="$sparkle_private_key" \
	"$appcast_script" \
	--archive "$archive_path" \
	--appcast "$appcast_path" \
	--version "$release_version" \
	--tag "$release_tag"
archive_sha256="$("$shasum_bin" -a 256 "$archive_path" | awk '{print $1}')"
printf '%s  %s\n' "$archive_sha256" "$ARCHIVE_NAME" >"$checksum_path"

"$artifact_validator" \
	--archive "$archive_path" \
	--appcast "$appcast_path" \
	--checksum "$checksum_path" \
	--repository "$CANONICAL_REPOSITORY" \
	--sparkle-version "$sparkle_version" \
	--tag "$release_tag" \
	--version "$release_version"

mv "$archive_path" "$output_dir/$ARCHIVE_NAME"
archive_landed=1
mv "$appcast_path" "$output_dir/$APPCAST_NAME"
appcast_landed=1
mv "$checksum_path" "$output_dir/$CHECKSUM_NAME"
checksum_landed=1
package_complete=1
printf 'Prepared signed release assets:\n  %s\n  %s\n  %s\n' \
	"$output_dir/$ARCHIVE_NAME" \
	"$output_dir/$APPCAST_NAME" \
	"$output_dir/$CHECKSUM_NAME"
