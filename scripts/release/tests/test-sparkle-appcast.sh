#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
APPCAST_TOOL="$ROOT_DIR/scripts/release/sparkle-appcast.py"
APPCAST_ASSERTION="$ROOT_DIR/scripts/release/tests/assert-sparkle-appcast.py"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

ARCHIVE="$TEST_ROOT/rsnap-aarch64-apple-darwin.zip"
APPCAST="$TEST_ROOT/appcast.xml"
FAKE_SIGN_UPDATE="$TEST_ROOT/sign_update"
SIGN_UPDATE_LOG="$TEST_ROOT/sign-update.log"
export SIGN_UPDATE_LOG

printf 'zip-bytes' >"$ARCHIVE"
cat >"$FAKE_SIGN_UPDATE" <<'SH'
#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" != "3" || "$1" != "--ed-key-file" || "$2" != "-" ]]; then
	echo "unexpected sign_update arguments" >&2
	exit 1
fi
read -r private_key
if [[ "$private_key" != "fake-private-key" ]]; then
	echo "unexpected private key input" >&2
	exit 1
fi
printf 'called\n' >>"$SIGN_UPDATE_LOG"
printf '%s\n' "$FAKE_SIGNATURE_OUTPUT"
SH
chmod +x "$FAKE_SIGN_UPDATE"

signature="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
valid_output="sparkle:edSignature=\"$signature\" length=\"9\""

run_appcast() {
	FAKE_SIGNATURE_OUTPUT="$1" \
	SPARKLE_PRIVATE_ED_KEY="fake-private-key" \
	SPARKLE_SIGN_UPDATE="$FAKE_SIGN_UPDATE" \
	SPARKLE_ARCHIVE_URL="${2:-}" \
	SPARKLE_RELEASE_NOTES_URL="${3:-}" \
		"$APPCAST_TOOL" \
		--archive "$ARCHIVE" \
		--appcast "$APPCAST" \
		--version "${4:-1.2.3}" \
		--tag "${5:-v1.2.3}"
}

: >"$SIGN_UPDATE_LOG"
run_appcast "$valid_output"
python3 "$APPCAST_ASSERTION" \
	"$APPCAST" \
	"$signature" \
	"https://github.com/acg-box/rsnap/releases/download/v1.2.3/rsnap-aarch64-apple-darwin.zip" \
	"https://github.com/acg-box/rsnap/releases/tag/v1.2.3"

loopback_archive_url="http://127.0.0.1:8123/update.zip?one=1&two=2"
loopback_notes_url="http://localhost:8123/notes.html?one=1&two=2"
run_appcast "$valid_output" "$loopback_archive_url" "$loopback_notes_url"
python3 "$APPCAST_ASSERTION" \
	"$APPCAST" \
	"$signature" \
	"$loopback_archive_url" \
	"$loopback_notes_url"

assert_failure_preserves_appcast() {
	local label="$1"
	shift
	printf 'sentinel' >"$APPCAST"
	if "$@" >/dev/null 2>&1; then
		echo "expected failure: $label" >&2
		exit 1
	fi
	if [[ "$(cat "$APPCAST")" != "sentinel" ]]; then
		echo "failure replaced the existing appcast: $label" >&2
		exit 1
	fi
}

assert_failure_preserves_appcast \
	"tag mismatch" \
	run_appcast "$valid_output" "" "" "1.2.3" "v1.2.4"

assert_failure_preserves_appcast \
	"non-canonical SemVer" \
	run_appcast "$valid_output" "" "" "01.2.3" "v01.2.3"

assert_failure_preserves_appcast \
	"wrong signed length" \
	run_appcast "sparkle:edSignature=\"$signature\" length=\"10\""

assert_failure_preserves_appcast \
	"extra signer output" \
	run_appcast "$valid_output
unexpected"

assert_failure_preserves_appcast \
	"malformed signature" \
	run_appcast 'sparkle:edSignature="AAAA" length="9"'

assert_failure_preserves_appcast \
	"non-canonical archive URL" \
	run_appcast "$valid_output" "https://example.com/update.zip"

if FAKE_SIGNATURE_OUTPUT="$valid_output" \
	SPARKLE_PRIVATE_ED_KEY="fake-private-key" \
	SPARKLE_SIGN_UPDATE="$FAKE_SIGN_UPDATE" \
		"$APPCAST_TOOL" \
		--archive "$ARCHIVE" \
		--appcast "$ARCHIVE" \
		--version "1.2.3" \
		--tag "v1.2.3" >/dev/null 2>&1; then
	echo "appcast generator accepted the archive as its output path" >&2
	exit 1
fi
if [[ "$(cat "$ARCHIVE")" != "zip-bytes" ]]; then
	echo "appcast generator replaced the signed release archive" >&2
	exit 1
fi

printf '' >"$ARCHIVE"
assert_failure_preserves_appcast \
	"empty archive" \
	run_appcast 'sparkle:edSignature="AAAA" length="1"'

echo "sparkle-appcast tests passed"
