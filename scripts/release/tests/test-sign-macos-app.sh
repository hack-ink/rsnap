#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SIGNER="$ROOT_DIR/scripts/release/sign-macos-app.sh"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

FAKE_CODESIGN="$TEST_ROOT/codesign"
CODESIGN_LOG="$TEST_ROOT/codesign.log"
export CODESIGN_LOG

cat >"$FAKE_CODESIGN" <<'SH'
#!/usr/bin/env bash
set -euo pipefail

{
	for argument in "$@"; do
		printf '%q ' "$argument"
	done
	printf '\n'
} >>"$CODESIGN_LOG"

if [[ " $* " == *" -dv "* ]]; then
	cat >&2 <<'DETAILS'
CodeDirectory v=20500 size=1 flags=0x10000(runtime) hashes=1+0 location=embedded
Authority=Apple Development: Test (RD3D4LH465)
TeamIdentifier=T54QFA7W2S
DETAILS
fi
if [[ " $* " == *" --entitlements :- "* ]]; then
	printf '%s\n' '<plist><dict/></plist>'
fi
SH
chmod +x "$FAKE_CODESIGN"

make_fixture() {
	local fixture_root="$1"
	local app="$fixture_root/Rsnap.app"
	local framework="$app/Contents/Frameworks/Sparkle.framework"

	mkdir -p \
		"$framework/Versions/B/XPCServices/Installer.xpc" \
		"$framework/Versions/B/XPCServices/Downloader.xpc" \
		"$framework/Versions/B/Updater.app"
	touch "$framework/Versions/B/Autoupdate"
	chmod +x "$framework/Versions/B/Autoupdate"
	ln -s B "$framework/Versions/Current"
	printf '%s\n' "$app"
}

assert_contains() {
	local text="$1"
	local expected="$2"
	if [[ "$text" != *"$expected"* ]]; then
		echo "expected log line to contain: $expected" >&2
		echo "actual: $text" >&2
		exit 1
	fi
}

app="$(make_fixture "$TEST_ROOT/development")"
keychain="$TEST_ROOT/test.keychain-db"
app_entitlements="$TEST_ROOT/app.entitlements"
touch "$keychain"
cat >"$app_entitlements" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>app-test</key><true/></dict></plist>
PLIST
: >"$CODESIGN_LOG"
CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$app" \
	--identity "Apple Development: Test" \
	--keychain "$keychain" \
	--entitlements "$app_entitlements" \
	--mode development

if [[ "$(wc -l <"$CODESIGN_LOG" | tr -d ' ')" != "13" ]]; then
	echo "expected six signing calls, six strict checks, and one deep verification" >&2
	exit 1
fi

installer_line="$(sed -n '1p' "$CODESIGN_LOG")"
downloader_line="$(sed -n '3p' "$CODESIGN_LOG")"
autoupdate_line="$(sed -n '5p' "$CODESIGN_LOG")"
updater_line="$(sed -n '7p' "$CODESIGN_LOG")"
framework_line="$(sed -n '9p' "$CODESIGN_LOG")"
app_line="$(sed -n '11p' "$CODESIGN_LOG")"
verify_line="$(sed -n '13p' "$CODESIGN_LOG")"

assert_contains "$installer_line" "Installer.xpc"
assert_contains "$downloader_line" "Downloader.xpc"
assert_contains "$autoupdate_line" "Autoupdate"
assert_contains "$updater_line" "Updater.app"
assert_contains "$framework_line" "Sparkle.framework"
assert_contains "$app_line" "Rsnap.app"
assert_contains "$downloader_line" "--preserve-metadata=entitlements"
assert_contains "$app_line" "--entitlements $app_entitlements"
assert_contains "$verify_line" "--verify --deep --strict"

while IFS= read -r signing_line; do
	assert_contains "$signing_line" "--options runtime"
	assert_contains "$signing_line" "--timestamp=none"
	assert_contains "$signing_line" "--keychain $keychain"
	if [[ "$signing_line" == *"--deep"* ]]; then
		echo "signing call must not use --deep: $signing_line" >&2
		exit 1
	fi
done < <(sed -n '1p;3p;5p;7p;9p;11p' "$CODESIGN_LOG")

: >"$CODESIGN_LOG"
CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$app" \
	--identity "Apple Development: Test (RD3D4LH465)" \
	--keychain "$keychain" \
	--mode release
if ! grep -q -- "-dv --verbose=4" "$CODESIGN_LOG"; then
	echo "release signer did not read back signed object details" >&2
	exit 1
fi

unsupported_app="$(make_fixture "$TEST_ROOT/unsupported")"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$unsupported_app" \
	--identity "Developer ID Application: Test" >/dev/null 2>&1; then
	echo "signer accepted an unsupported release identity" >&2
	exit 1
fi
if [[ -s "$CODESIGN_LOG" ]]; then
	echo "signer called codesign before rejecting an unsupported identity" >&2
	exit 1
fi

wrong_team_app="$(make_fixture "$TEST_ROOT/wrong-team")"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$wrong_team_app" \
	--identity "Apple Development: Test (OTHERTEAM1)" \
	--keychain "$keychain" >/dev/null 2>&1; then
	echo "signer accepted an unexpected Personal Team" >&2
	exit 1
fi
if [[ -s "$CODESIGN_LOG" ]]; then
	echo "signer called codesign before rejecting an unexpected team" >&2
	exit 1
fi

wrong_team_details_app="$(make_fixture "$TEST_ROOT/wrong-team-details")"
wrong_team_codesign="$TEST_ROOT/wrong-team-codesign"
sed 's/TeamIdentifier=T54QFA7W2S/TeamIdentifier=RD3D4LH465/' \
	"$FAKE_CODESIGN" >"$wrong_team_codesign"
chmod +x "$wrong_team_codesign"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$wrong_team_codesign" "$SIGNER" \
	--app "$wrong_team_details_app" \
	--identity "Apple Development: Test (RD3D4LH465)" \
	--keychain "$keychain" >/dev/null 2>&1; then
	echo "signer accepted the identity suffix as the code-signing TeamIdentifier" >&2
	exit 1
fi
unsafe_target_app="$(make_fixture "$TEST_ROOT/unsafe-target")"
rm "$unsafe_target_app/Contents/Frameworks/Sparkle.framework/Versions/Current"
ln -s $'B\nhidden' "$unsafe_target_app/Contents/Frameworks/Sparkle.framework/Versions/Current"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$unsafe_target_app" \
	--identity "Apple Development: Test" \
	--mode development >/dev/null 2>&1; then
	echo "signer accepted a control character in Versions/Current" >&2
	exit 1
fi

outside="$TEST_ROOT/outside"
mkdir -p "$outside"
escaped_app="$(make_fixture "$TEST_ROOT/escaped")"
rm "$escaped_app/Contents/Frameworks/Sparkle.framework/Versions/Current"
ln -s "$outside" "$escaped_app/Contents/Frameworks/Sparkle.framework/Versions/Current"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$escaped_app" \
	--identity "Apple Development: Test" \
	--mode development >/dev/null 2>&1; then
	echo "signer accepted a Versions/Current symlink outside Sparkle.framework" >&2
	exit 1
fi
if [[ -s "$CODESIGN_LOG" ]]; then
	echo "signer called codesign before rejecting an unsafe framework layout" >&2
	exit 1
fi

extra_alias_app="$(make_fixture "$TEST_ROOT/extra-alias")"
ln -s B "$extra_alias_app/Contents/Frameworks/Sparkle.framework/Versions/Previous"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$extra_alias_app" \
	--identity "Apple Development: Test" \
	--mode development >/dev/null 2>&1; then
	echo "signer accepted an extra Sparkle Versions alias" >&2
	exit 1
fi
if [[ -s "$CODESIGN_LOG" ]]; then
	echo "signer called codesign before rejecting an extra Versions alias" >&2
	exit 1
fi

unknown_app="$(make_fixture "$TEST_ROOT/unknown")"
mkdir -p \
	"$unknown_app/Contents/Frameworks/Sparkle.framework/Versions/B/Unexpected.app"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$unknown_app" \
	--identity "Apple Development: Test" \
	--mode development >/dev/null 2>&1; then
	echo "signer accepted an unknown nested code bundle" >&2
	exit 1
fi
if [[ -s "$CODESIGN_LOG" ]]; then
	echo "signer called codesign before rejecting an unknown graph" >&2
	exit 1
fi
incomplete_app="$(make_fixture "$TEST_ROOT/incomplete")"
rm -rf "$incomplete_app/Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app"
: >"$CODESIGN_LOG"
if CODESIGN_BIN="$FAKE_CODESIGN" "$SIGNER" \
	--app "$incomplete_app" \
	--identity "Apple Development: Test" \
	--mode development >/dev/null 2>&1; then
	echo "signer accepted a Sparkle framework with a missing nested component" >&2
	exit 1
fi
if [[ -s "$CODESIGN_LOG" ]]; then
	echo "signer started signing before validating every required component" >&2
	exit 1
fi

echo "sign-macos-app tests passed"
