#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMMON_ROOT="$(cd "$(git -C "$ROOT_DIR" rev-parse --git-common-dir)/.." && pwd)"
WORK_ROOT="${RSNAP_SPARKLE_SMOKE_WORK_ROOT:-$COMMON_ROOT/target/rsnap-sparkle-update-smoke}"
ARCHIVE_NAME="rsnap-aarch64-apple-darwin.zip"
OLD_VERSION="${RSNAP_SPARKLE_SMOKE_OLD_VERSION:-0.1.2}"
NEW_VERSION="${RSNAP_SPARKLE_SMOKE_NEW_VERSION:-99.0.0}"
HOST="${RSNAP_SPARKLE_SMOKE_HOST:-127.0.0.1}"
PORT="${RSNAP_SPARKLE_SMOKE_PORT:-}"
PREPARE_ONLY=0
SELF_CHECK=0

usage() {
	cat <<'USAGE'
Usage: scripts/smoke/sparkle-update-local.sh [--prepare-only] [--self-check]

Builds a local Sparkle update fixture:
  1. generate temporary Sparkle test keys
  2. stage an old Rsnap.app pointing at a local appcast
  3. stage a higher-version Rsnap.app and zip it
  4. sign the zip and write appcast.xml
  5. serve the appcast locally and launch the old app

The final version readback is manually gated so the operator can observe the automatic
install-and-relaunch path.

Options:
  --prepare-only  build fixtures and print paths without launching the app or server
  --self-check    run a fast appcast-generation smoke without building apps
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
		--prepare-only)
			PREPARE_ONLY=1
			shift
			;;
		--self-check)
			SELF_CHECK=1
			shift
			;;
		-h|--help)
			usage
			exit 0
			;;
		*)
			echo "error: unknown argument: $1" >&2
			usage >&2
			exit 2
			;;
	esac
done

generate_test_keys() {
	swift - <<'SWIFT'
import CryptoKit
import Foundation

let privateKey = Curve25519.Signing.PrivateKey()
print(privateKey.rawRepresentation.base64EncodedString())
print(privateKey.publicKey.rawRepresentation.base64EncodedString())
SWIFT
}

choose_port() {
	python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}

run_self_check() {
	local tmpdir archive appcast fake_sign_update
	tmpdir="$(mktemp -d)"
	archive="$tmpdir/$ARCHIVE_NAME"
	appcast="$tmpdir/appcast.xml"
	fake_sign_update="$tmpdir/sign_update"
	printf 'zip-bytes' >"$archive"
	cat >"$fake_sign_update" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
file=""
while [[ $# -gt 0 ]]; do
	case "$1" in
		--ed-key-file)
			read -r _private_key
			shift 2
			;;
		*)
			file="$1"
			shift
			;;
	esac
done
size="$(wc -c <"$file" | tr -d ' ')"
printf 'sparkle:edSignature="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" length="%s"\n' "$size"
SH
	chmod +x "$fake_sign_update"
	RSNAP_SPARKLE_PRIVATE_ED_KEY="fake-private-key" \
		SPARKLE_SIGN_UPDATE="$fake_sign_update" \
		SPARKLE_ARCHIVE_URL="http://127.0.0.1:9/$ARCHIVE_NAME" \
		SPARKLE_RELEASE_NOTES_URL="http://127.0.0.1:9/release-notes.html" \
		"$ROOT_DIR/scripts/release/sparkle-appcast.py" \
		--archive "$archive" \
		--appcast "$appcast" \
		--version "99.0.0" \
		--tag "v99.0.0"
	python3 - "$appcast" <<'PY'
import sys
import xml.etree.ElementTree as ET

path = sys.argv[1]
text = open(path, encoding="utf-8").read()
assert "http://127.0.0.1:9/rsnap-aarch64-apple-darwin.zip" in text
assert 'sparkle:edSignature="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="' in text
assert 'length="9"' in text
assert ET.parse(path).getroot().tag == "rss"
print("sparkle update local self-check ok")
PY
}

if [[ "$SELF_CHECK" == "1" ]]; then
	run_self_check
	exit 0
fi

if [[ -z "$PORT" ]]; then
	PORT="$(choose_port)"
fi

SERVER_DIR="$WORK_ROOT/server"
OLD_STAGE_DIR="$WORK_ROOT/old"
NEW_STAGE_DIR="$WORK_ROOT/new"
LOG_PATH="$WORK_ROOT/http.log"
APPCAST_URL="http://$HOST:$PORT/appcast.xml"
ARCHIVE_URL="http://$HOST:$PORT/$ARCHIVE_NAME"
RELEASE_NOTES_URL="http://$HOST:$PORT/release-notes.html"

sparkle_key_output="$(generate_test_keys)"
RSNAP_SPARKLE_PRIVATE_ED_KEY="$(printf '%s\n' "$sparkle_key_output" | sed -n '1p')"
SPARKLE_PUBLIC_ED_KEY="$(printf '%s\n' "$sparkle_key_output" | sed -n '2p')"

rm -rf "$WORK_ROOT"
mkdir -p "$SERVER_DIR" "$OLD_STAGE_DIR" "$NEW_STAGE_DIR"

cat >"$SERVER_DIR/release-notes.html" <<HTML
<!doctype html>
<html><body><h1>Rsnap $NEW_VERSION Smoke Update</h1></body></html>
HTML

echo "Building new Rsnap.app $NEW_VERSION..."
RSNAP_NATIVE_HOST_STAGE_DIR="$NEW_STAGE_DIR" \
	RSNAP_NATIVE_HOST_APP_VERSION="$NEW_VERSION" \
	RSNAP_SPARKLE_APPCAST_URL="$APPCAST_URL" \
	RSNAP_SPARKLE_PUBLIC_ED_KEY="$SPARKLE_PUBLIC_ED_KEY" \
	RSNAP_NATIVE_HOST_FORCE_REBUILD=1 \
	"$ROOT_DIR/scripts/build_and_run.sh" stage

ditto -c -k --sequesterRsrc --keepParent \
	"$NEW_STAGE_DIR/Rsnap.app" \
	"$SERVER_DIR/$ARCHIVE_NAME"

RSNAP_SPARKLE_PRIVATE_ED_KEY="$RSNAP_SPARKLE_PRIVATE_ED_KEY" \
	SPARKLE_ARCHIVE_URL="$ARCHIVE_URL" \
	SPARKLE_RELEASE_NOTES_URL="$RELEASE_NOTES_URL" \
	"$ROOT_DIR/scripts/release/sparkle-appcast.py" \
	--archive "$SERVER_DIR/$ARCHIVE_NAME" \
	--appcast "$SERVER_DIR/appcast.xml" \
	--version "$NEW_VERSION" \
	--tag "v$NEW_VERSION"

echo "Building old Rsnap.app $OLD_VERSION..."
RSNAP_NATIVE_HOST_STAGE_DIR="$OLD_STAGE_DIR" \
	RSNAP_NATIVE_HOST_APP_VERSION="$OLD_VERSION" \
	RSNAP_SPARKLE_APPCAST_URL="$APPCAST_URL" \
	RSNAP_SPARKLE_PUBLIC_ED_KEY="$SPARKLE_PUBLIC_ED_KEY" \
	RSNAP_NATIVE_HOST_FORCE_REBUILD=1 \
	"$ROOT_DIR/scripts/build_and_run.sh" stage

old_app="$OLD_STAGE_DIR/Rsnap.app"
actual_old_version="$(plutil -extract CFBundleVersion raw "$old_app/Contents/Info.plist")"
if [[ "$actual_old_version" != "$OLD_VERSION" ]]; then
	echo "error: old app version mismatch: expected $OLD_VERSION, got $actual_old_version" >&2
	exit 1
fi

if [[ "$PREPARE_ONLY" == "1" ]]; then
	cat <<EOF
Sparkle update fixture prepared.

Old app: $old_app
New archive: $SERVER_DIR/$ARCHIVE_NAME
Appcast: $SERVER_DIR/appcast.xml
Appcast URL: $APPCAST_URL
Public key: $SPARKLE_PUBLIC_ED_KEY
EOF
	exit 0
fi

python3 -m http.server "$PORT" --bind "$HOST" --directory "$SERVER_DIR" >"$LOG_PATH" 2>&1 &
server_pid="$!"
cleanup() {
	kill "$server_pid" >/dev/null 2>&1 || true
}
trap cleanup EXIT

sleep 0.5
curl -fsSL "$APPCAST_URL" >/dev/null

cat <<EOF
Local Sparkle update smoke is ready.

Old app: $old_app
Old version: $OLD_VERSION
New version: $NEW_VERSION
Appcast URL: $APPCAST_URL
HTTP log: $LOG_PATH

Next manual steps:
  1. Wait for Rsnap to detect, install, and relaunch from the local appcast.
  2. Return here and press Enter.
EOF

pkill -f "$old_app/Contents/MacOS/RsnapNativeHost" >/dev/null 2>&1 || true
/usr/bin/open -n "$old_app"
read -r -p "Press Enter after Rsnap finishes the automatic install and relaunch..."

actual_version="$(plutil -extract CFBundleVersion raw "$old_app/Contents/Info.plist")"
if [[ "$actual_version" != "$NEW_VERSION" ]]; then
	echo "error: app did not update in place: expected $NEW_VERSION, got $actual_version" >&2
	exit 1
fi

echo "Sparkle update smoke passed: $OLD_VERSION -> $NEW_VERSION"
