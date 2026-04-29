#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
APP_NAME="rsnap"
EXECUTABLE_NAME="RsnapNativeHost"
BUNDLE_ID="ink.hack.rsnap"
MIN_SYSTEM_VERSION="14.0"
DEFAULT_SIGN_IDENTITY="x@acg.box"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE_DIR="$ROOT_DIR/native/macos-host"
COMMON_ROOT="$(cd "$(git -C "$ROOT_DIR" rev-parse --git-common-dir)/.." && pwd)"
STAGE_DIR="${RSNAP_NATIVE_HOST_STAGE_DIR:-$COMMON_ROOT/target/rsnap-native-host}"
APP_BUNDLE="$STAGE_DIR/$APP_NAME.app"
APP_CONTENTS="$APP_BUNDLE/Contents"
APP_MACOS="$APP_CONTENTS/MacOS"
APP_RESOURCES="$APP_CONTENTS/Resources"
APP_BINARY="$APP_MACOS/$EXECUTABLE_NAME"
APP_SOURCE_BINARY_CACHE="$STAGE_DIR/.${EXECUTABLE_NAME}.source-bin"
STAGE_FINGERPRINT_FILE="$STAGE_DIR/.stage-fingerprint"
INFO_PLIST="$APP_CONTENTS/Info.plist"
APP_ICON_SOURCE="$ROOT_DIR/assets/app-icon/generated/app-icon.icns"
APP_ICON_NAME="AppIcon.icns"
STATUS_ICON_SOURCE="$ROOT_DIR/assets/tray-icon/generated/tray-icon-template.png"
STATUS_ICON_NAME="StatusBarIcon.png"
BUILD_ROOT=""
BUILD_BINARY=""
SWIFT_BUILD_FLAGS=()
RUST_BUILD_ARGS=(-p rsnap-host-ffi)
RESOLVED_SIGN_IDENTITY=""
STAGED_APP_DIRTY=0

case "$MODE" in
	run|logs|--logs|telemetry|--telemetry|verify|--verify)
		default_rust_profile="final-release"
		default_swift_configuration="release"
		;;
	*)
		default_rust_profile="debug"
		default_swift_configuration="debug"
		;;
esac

RUST_PROFILE="${RSNAP_NATIVE_HOST_RUST_PROFILE:-$default_rust_profile}"
SWIFT_CONFIGURATION="${RSNAP_NATIVE_HOST_SWIFT_CONFIGURATION:-$default_swift_configuration}"
if [[ "$RUST_PROFILE" == "debug" ]]; then
	RUST_LIB_DIR="$ROOT_DIR/target/debug"
else
	RUST_BUILD_ARGS+=(--profile "$RUST_PROFILE")
	RUST_LIB_DIR="$ROOT_DIR/target/$RUST_PROFILE"
fi

if [[ "$SWIFT_CONFIGURATION" == "release" ]]; then
	SWIFT_BUILD_FLAGS=(-c release)
fi

APP_VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)"
APP_VERSION="${APP_VERSION:-0.1.0}"

relink_native_host_if_missing() {
	local product_dir link_file swift_frameworks sdk swiftc
	product_dir="$BUILD_ROOT/$EXECUTABLE_NAME.product"
	link_file="$product_dir/Objects.LinkFileList"
	swift_frameworks="/Library/Developer/CommandLineTools/Library/Developer/Frameworks"
	sdk="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk"
	swiftc="/Library/Developer/CommandLineTools/usr/bin/swiftc"

	if [[ ! -f "$link_file" ]]; then
		mkdir -p "$product_dir"
		{
			find "$BUILD_ROOT/RsnapHostBridge.build" -maxdepth 1 -name '*.o' -print 2>/dev/null
			find "$BUILD_ROOT/RsnapNativeHostKit.build" -maxdepth 1 -name '*.o' -print 2>/dev/null
			find "$BUILD_ROOT/$EXECUTABLE_NAME.build" -maxdepth 1 -name '*.o' -print 2>/dev/null
		} | sort >"$link_file"
	fi

	if [[ ! -s "$link_file" ]]; then
		echo "error: failed to recover link inputs for $EXECUTABLE_NAME under $BUILD_ROOT" >&2
		exit 1
	fi

	"$swiftc" \
		-L "$BUILD_ROOT" \
		-o "$BUILD_BINARY" \
		-module-name "$EXECUTABLE_NAME" \
		-Xlinker -no_warn_duplicate_libraries \
		-emit-executable \
		-Xlinker -alias \
		-Xlinker "_${EXECUTABLE_NAME}_main" \
		-Xlinker _main \
		-Xlinker -rpath \
		-Xlinker @loader_path \
		@"$link_file" \
		-Xlinker -rpath \
		-Xlinker /Library/Developer/CommandLineTools/usr/lib/swift-6.2/macosx \
		-target arm64-apple-macosx14.0 \
		-framework AppKit \
		-framework Vision \
		-L "$RUST_LIB_DIR" \
		-lrsnap_host_ffi \
		-Xlinker -add_ast_path \
		-Xlinker "$BUILD_ROOT/Modules/RsnapHostBridge.swiftmodule" \
		-Xlinker -add_ast_path \
		-Xlinker "$BUILD_ROOT/Modules/RsnapNativeHost.swiftmodule" \
		-Xlinker -add_ast_path \
		-Xlinker "$BUILD_ROOT/Modules/RsnapNativeHostKit.swiftmodule" \
		-I "$swift_frameworks" \
		-L "$swift_frameworks" \
		-plugin-path /Library/Developer/CommandLineTools/usr/lib/swift/host/plugins/testing \
		-sdk "$sdk" \
		-g

	codesign --force --sign - --entitlements "$BUILD_ROOT/$EXECUTABLE_NAME-entitlement.plist" "$BUILD_BINARY"
}

copy_if_changed() {
	local source="$1"
	local destination="$2"
	if [[ -f "$destination" ]] && cmp -s "$source" "$destination"; then
		return 1
	fi
	mkdir -p "$(dirname "$destination")"
	cp "$source" "$destination"
	return 0
}

sync_bundle_dir() {
	local source_dir="$1"
	local destination_dir="$2"
	if [[ ! -d "$source_dir" ]]; then
		return 1
	fi

	mkdir -p "$(dirname "$destination_dir")"
	rm -rf "$destination_dir"
	cp -R "$source_dir" "$destination_dir"
	return 0
}

write_if_changed() {
	local destination="$1"
	local contents="$2"
	local tmp
	tmp="$(mktemp)"
	printf '%s' "$contents" >"$tmp"
	if [[ -f "$destination" ]] && cmp -s "$tmp" "$destination"; then
		rm -f "$tmp"
		return 1
	fi
	mkdir -p "$(dirname "$destination")"
	mv "$tmp" "$destination"
	return 0
}

stage_app_bundle() {
	mkdir -p "$APP_MACOS" "$APP_RESOURCES"
	if [[ ! -x "$APP_BINARY" ]] || copy_if_changed "$BUILD_BINARY" "$APP_SOURCE_BINARY_CACHE"; then
		mkdir -p "$(dirname "$APP_SOURCE_BINARY_CACHE")"
		cp "$BUILD_BINARY" "$APP_BINARY"
		STAGED_APP_DIRTY=1
		chmod +x "$APP_BINARY"
	fi

	if [[ -f "$APP_ICON_SOURCE" ]]; then
		if copy_if_changed "$APP_ICON_SOURCE" "$APP_RESOURCES/$APP_ICON_NAME"; then
			STAGED_APP_DIRTY=1
		fi
	fi
	if [[ -f "$STATUS_ICON_SOURCE" ]]; then
		if copy_if_changed "$STATUS_ICON_SOURCE" "$APP_RESOURCES/$STATUS_ICON_NAME"; then
			STAGED_APP_DIRTY=1
		fi
	fi

	local resource_bundle
	while IFS= read -r resource_bundle; do
		[[ -n "$resource_bundle" ]] || continue
		if sync_bundle_dir "$resource_bundle" "$APP_RESOURCES/$(basename "$resource_bundle")"; then
			STAGED_APP_DIRTY=1
		fi
	done < <(find "$BUILD_ROOT" -maxdepth 1 -name '*.bundle' -type d | sort)

	local info_plist_contents
	info_plist_contents="$(cat <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>$EXECUTABLE_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$APP_VERSION</string>
  <key>CFBundleVersion</key>
  <string>$APP_VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_SYSTEM_VERSION</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
PLIST
)"

	if [[ -f "$APP_RESOURCES/$APP_ICON_NAME" ]]; then
		info_plist_contents+="$(cat <<PLIST
  <key>CFBundleIconFile</key>
  <string>${APP_ICON_NAME%.icns}</string>
PLIST
)"
	fi

	info_plist_contents+="$(cat <<'PLIST'
</dict>
</plist>
PLIST
)"

	if write_if_changed "$INFO_PLIST" "$info_plist_contents"; then
		STAGED_APP_DIRTY=1
	fi
}

resolve_signing_identity() {
	local requested_identity identity_list identity

	requested_identity="${RSNAP_NATIVE_HOST_SIGN_IDENTITY:-$DEFAULT_SIGN_IDENTITY}"
	identity_list="$(security find-identity -v -p codesigning 2>/dev/null || true)"
	if [[ -n "$requested_identity" ]]; then
		while IFS= read -r line; do
			identity="${line#*\"}"
			identity="${identity%%\"*}"
			if [[ -n "$identity" && "$identity" == *"$requested_identity"* ]]; then
				RESOLVED_SIGN_IDENTITY="$identity"
				return 0
			fi
		done <<<"$identity_list"
	fi

	while IFS= read -r line; do
		identity="${line#*\"}"
		identity="${identity%%\"*}"
		if [[ -n "$identity" && "$identity" == Apple\ Development:* ]]; then
			RESOLVED_SIGN_IDENTITY="$identity"
			return 0
		fi
	done <<<"$identity_list"

	return 1
}

sign_staged_app_bundle() {
	local entitlements_arg=() requested_identity
	requested_identity="${RSNAP_NATIVE_HOST_SIGN_IDENTITY:-$DEFAULT_SIGN_IDENTITY}"
	if [[ "$STAGED_APP_DIRTY" != "1" ]] && codesign --verify --deep --strict "$APP_BUNDLE" >/dev/null 2>&1; then
		return
	fi
	if [[ -f "$BUILD_ROOT/$EXECUTABLE_NAME-entitlement.plist" ]]; then
		entitlements_arg=(--entitlements "$BUILD_ROOT/$EXECUTABLE_NAME-entitlement.plist")
	fi
	if resolve_signing_identity; then
		codesign \
			--force \
			--deep \
			--options runtime \
			--sign "$RESOLVED_SIGN_IDENTITY" \
			"${entitlements_arg[@]}" \
			"$APP_BUNDLE"
		return
	fi

	echo "error: no valid macOS codesigning identity matching \"$requested_identity\" was found." >&2
	echo "error: import the real signing certificate or set RSNAP_NATIVE_HOST_SIGN_IDENTITY to a valid identity." >&2
	echo "error: rsnap native host staging requires a stable codesigning identity." >&2
	exit 1
}

compute_stage_fingerprint() {
	python3 - "$ROOT_DIR" "$RUST_PROFILE" "$SWIFT_CONFIGURATION" "$APP_VERSION" <<'PY'
import hashlib
import os
import sys

root, rust_profile, swift_configuration, app_version = sys.argv[1:]
targets = [
	"Cargo.toml",
	"Cargo.lock",
	"Makefile.toml",
	"README.md",
	"apps/rsnap",
	"assets/app-icon",
	"assets/tray-icon",
	"native/macos-host",
	"packages/rsnap-capture-core",
	"packages/rsnap-host-ffi",
	"scripts/build_and_run.sh",
]
skip_dirs = {".git", ".worktrees", "target", ".build"}

hasher = hashlib.sha256()
for value in (rust_profile, swift_configuration, app_version):
	hasher.update(value.encode("utf-8"))
	hasher.update(b"\0")

def update_file(path: str) -> None:
	rel = os.path.relpath(path, root)
	hasher.update(rel.encode("utf-8"))
	hasher.update(b"\0")
	with open(path, "rb") as handle:
		for chunk in iter(lambda: handle.read(1 << 20), b""):
			hasher.update(chunk)

for target in targets:
	abs_target = os.path.join(root, target)
	if not os.path.exists(abs_target):
		continue
	if os.path.isfile(abs_target):
		update_file(abs_target)
		continue
	for dirpath, dirnames, filenames in os.walk(abs_target):
		dirnames[:] = [name for name in sorted(dirnames) if name not in skip_dirs]
		for filename in sorted(filenames):
			update_file(os.path.join(dirpath, filename))

print(hasher.hexdigest())
PY
}

write_stage_fingerprint() {
	local fingerprint
	fingerprint="$(compute_stage_fingerprint)"
	mkdir -p "$(dirname "$STAGE_FINGERPRINT_FILE")"
	printf '%s\n' "$fingerprint" >"$STAGE_FINGERPRINT_FILE"
}

staged_bundle_is_current() {
	[[ -x "$APP_BINARY" ]] || return 1
	[[ -f "$STAGE_FINGERPRINT_FILE" ]] || return 1
	codesign --verify --deep --strict "$APP_BUNDLE" >/dev/null 2>&1 || return 1

	local expected actual
	expected="$(compute_stage_fingerprint)"
	actual="$(tr -d '\n' <"$STAGE_FINGERPRINT_FILE")"
	[[ -n "$expected" && "$expected" == "$actual" ]]
}

terminate_running_host() {
	local remaining_pids=""
	pkill -x "$EXECUTABLE_NAME" >/dev/null 2>&1 || true
	for ((attempt = 0; attempt < 20; attempt++)); do
		remaining_pids="$(pgrep -x "$EXECUTABLE_NAME" || true)"
		[[ -z "$remaining_pids" ]] && return 0
		sleep 0.1
	done
	while IFS= read -r pid; do
		[[ -n "$pid" ]] || continue
		kill -9 "$pid" >/dev/null 2>&1 || true
	done <<<"$remaining_pids"
}

if [[ "$MODE" != "stage" ]]; then
	terminate_running_host
fi

if [[ "${RSNAP_NATIVE_HOST_FORCE_REBUILD:-0}" != "1" ]] && staged_bundle_is_current; then
	BUILD_ROOT=""
	BUILD_BINARY="$APP_BINARY"
else
	cargo build "${RUST_BUILD_ARGS[@]}"
	if [[ "${RSNAP_NATIVE_HOST_SWIFT_CLEAN:-0}" == "1" ]]; then
		swift package --package-path "$PACKAGE_DIR" clean
	fi
	RSNAP_HOST_FFI_LIB_DIR="$RUST_LIB_DIR" \
		swift build --package-path "$PACKAGE_DIR" "${SWIFT_BUILD_FLAGS[@]}" --product "$EXECUTABLE_NAME"
	BUILD_ROOT="$(RSNAP_HOST_FFI_LIB_DIR="$RUST_LIB_DIR" swift build --package-path "$PACKAGE_DIR" "${SWIFT_BUILD_FLAGS[@]}" --show-bin-path)"
	BUILD_BINARY="$BUILD_ROOT/$EXECUTABLE_NAME"

	if [[ ! -x "$BUILD_BINARY" ]]; then
		relink_native_host_if_missing
	fi

	stage_app_bundle
	sign_staged_app_bundle
	write_stage_fingerprint
fi

open_app() {
	/usr/bin/open "$APP_BUNDLE"
}

case "$MODE" in
	stage|--stage)
		;;
	run)
		open_app
		;;
	--debug|debug)
		lldb -- "$APP_BINARY"
		;;
	--logs|logs)
		open_app
		/usr/bin/log stream --info --style compact --predicate "process == \"$EXECUTABLE_NAME\""
		;;
	--telemetry|telemetry)
		open_app
		RSNAP_TELEMETRY_BUNDLE_ID="$BUNDLE_ID" \
			RSNAP_TELEMETRY_PROCESS="$EXECUTABLE_NAME" \
			"$ROOT_DIR/scripts/telemetry/native-host.sh" stream
		;;
	--verify|verify)
		open_app
		sleep 1
		pgrep -x "$EXECUTABLE_NAME" >/dev/null
		;;
	*)
		echo "usage: $0 [run|stage|--debug|--logs|--telemetry|--verify]" >&2
		exit 2
		;;
esac
