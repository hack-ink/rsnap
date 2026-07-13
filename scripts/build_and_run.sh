#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
APP_NAME="Rsnap"
EXECUTABLE_NAME="RsnapNativeHost"
BUNDLE_ID="ink.hack.rsnap"
MIN_SYSTEM_VERSION="14.0"
DEFAULT_SIGN_IDENTITY="x@acg.box"
DEFAULT_SPARKLE_PUBLIC_ED_KEY="X2EaTv6mCzkYxz75Hh+ldMkKlpzNlHRg5l7Kn9ke8Ow="

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE_DIR="$ROOT_DIR/native/macos-host"
COMMON_ROOT="$(cd "$(git -C "$ROOT_DIR" rev-parse --git-common-dir)/.." && pwd)"
STAGE_DIR="${RSNAP_NATIVE_HOST_STAGE_DIR:-$COMMON_ROOT/target/rsnap-native-host}"
APP_BUNDLE="$STAGE_DIR/$APP_NAME.app"
APP_CONTENTS="$APP_BUNDLE/Contents"
APP_MACOS="$APP_CONTENTS/MacOS"
APP_RESOURCES="$APP_CONTENTS/Resources"
APP_FRAMEWORKS="$APP_CONTENTS/Frameworks"
APP_BINARY="$APP_MACOS/$EXECUTABLE_NAME"
APP_SOURCE_BINARY_CACHE="$STAGE_DIR/.${EXECUTABLE_NAME}.source-bin"
STAGE_FINGERPRINT_FILE="$STAGE_DIR/.stage-fingerprint"
INFO_PLIST="$APP_CONTENTS/Info.plist"
APP_ICON_SOURCE="$ROOT_DIR/assets/app-icon/generated/app-icon.icns"
APP_ICON_NAME="AppIcon.icns"
STATUS_ICON_SOURCE="$ROOT_DIR/assets/tray-icon/generated/tray-icon-template.png"
STATUS_ICON_NAME="StatusBarIcon.png"
SPARKLE_APPCAST_URL="${RSNAP_SPARKLE_APPCAST_URL:-https://github.com/acgxv/rsnap/releases/latest/download/appcast.xml}"
# The public update key is safe to ship in source. The override exists only for
# local Sparkle smoke tests that generate a disposable key pair and appcast.
SPARKLE_PUBLIC_ED_KEY="${RSNAP_SPARKLE_PUBLIC_ED_KEY:-$DEFAULT_SPARKLE_PUBLIC_ED_KEY}"
BUILD_ROOT=""
BUILD_BINARY=""
SWIFT_BUILD_FLAGS=()
RUST_BUILD_ARGS=(-p rsnap-host-ffi)
RESOLVED_SIGN_IDENTITY=""
STAGED_APP_DIRTY=0

RUST_PROFILE="final-release"
SWIFT_CONFIGURATION="release"
if [[ -z "${RSNAP_NATIVE_HOST_SWIFT_CLEAN:-}" ]]; then
	if [[ "$SWIFT_CONFIGURATION" == "release" ]]; then
		# SwiftPM can reuse stale release objects across local worktree rebuilds. Prefer a
		# correct staged native host by default; set RSNAP_NATIVE_HOST_SWIFT_CLEAN=0 for
		# explicit incremental release rebuilds.
		RSNAP_NATIVE_HOST_SWIFT_CLEAN=1
	else
		RSNAP_NATIVE_HOST_SWIFT_CLEAN=0
	fi
fi
if [[ "$RUST_PROFILE" == "debug" ]]; then
	RUST_LIB_DIR="$ROOT_DIR/target/debug"
else
	RUST_BUILD_ARGS+=(--profile "$RUST_PROFILE")
	RUST_LIB_DIR="$ROOT_DIR/target/$RUST_PROFILE"
fi

if [[ "$SWIFT_CONFIGURATION" == "release" ]]; then
	SWIFT_BUILD_FLAGS=(-c release)
fi

APP_VERSION="${RSNAP_NATIVE_HOST_APP_VERSION:-}"
if [[ -z "$APP_VERSION" ]]; then
	APP_VERSION="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)"
fi
APP_VERSION="${APP_VERSION:-0.2.5}"

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
		-framework ServiceManagement \
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

sync_framework_dir() {
	local source_dir="$1"
	local destination_dir="$2"
	if [[ ! -d "$source_dir" ]]; then
		return 1
	fi

	mkdir -p "$(dirname "$destination_dir")"
	rm -rf "$destination_dir"
	ditto "$source_dir" "$destination_dir"
	return 0
}

stage_sparkle_framework() {
	local source_framework=""
	if [[ -d "$PACKAGE_DIR/.build/artifacts" ]]; then
		source_framework="$(
			find "$PACKAGE_DIR/.build/artifacts" \
				-type d \
				-name 'Sparkle.framework' \
				-print \
				-quit
		)"
	fi
	if [[ -z "$source_framework" ]]; then
		echo "error: Sparkle.framework was not found in native/macos-host/.build/artifacts." >&2
		echo "error: run swift package resolve/build for the Sparkle SwiftPM artifact first." >&2
		exit 1
	fi
	if sync_framework_dir "$source_framework" "$APP_FRAMEWORKS/Sparkle.framework"; then
		STAGED_APP_DIRTY=1
	fi
}

staged_app_rpaths() {
	otool -l "$APP_BINARY" | awk '
		$1 == "cmd" && $2 == "LC_RPATH" {
			in_rpath = 1
			next
		}
		in_rpath && $1 == "path" {
			sub(/^[[:space:]]*path /, "")
			sub(/[[:space:]]+\(offset [0-9]+\)$/, "")
			print
			in_rpath = 0
		}
	'
}

is_local_toolchain_rpath() {
	case "$1" in
		/Applications/*.app/Contents/Developer/Toolchains/*/usr/lib/swift*/macosx | \
			/Library/Developer/CommandLineTools/usr/lib/swift*/macosx | \
			/Users/*/Applications/*.app/Contents/Developer/Toolchains/*/usr/lib/swift*/macosx)
			return 0
			;;
		*)
			return 1
			;;
	esac
}

sanitize_staged_app_rpaths() {
	local rpath
	while IFS= read -r rpath; do
		[[ -n "$rpath" ]] || continue
		if is_local_toolchain_rpath "$rpath"; then
			install_name_tool -delete_rpath "$rpath" "$APP_BINARY"
			STAGED_APP_DIRTY=1
		fi
	done < <(staged_app_rpaths)
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

canonicalize_app_bundle_name() {
	local desired_name="$APP_NAME.app"
	local existing_bundle existing_name tmp_bundle

	mkdir -p "$STAGE_DIR"
	while IFS= read -r existing_bundle; do
		[[ -n "$existing_bundle" ]] || continue
		existing_name="$(basename "$existing_bundle")"
		[[ "$existing_name" == "$desired_name" ]] && continue

		if [[ -e "$APP_BUNDLE" ]]; then
			rm -rf "$existing_bundle"
		else
			tmp_bundle="$STAGE_DIR/.$desired_name.rename.$$"
			rm -rf "$tmp_bundle"
			mv "$existing_bundle" "$tmp_bundle"
			mv "$tmp_bundle" "$APP_BUNDLE"
		fi
		STAGED_APP_DIRTY=1
	done < <(find "$STAGE_DIR" -maxdepth 1 -type d -iname "$desired_name" -print)
}

stage_app_bundle() {
	canonicalize_app_bundle_name
	mkdir -p "$APP_MACOS" "$APP_RESOURCES" "$APP_FRAMEWORKS"
	if [[ ! -x "$APP_BINARY" ]] || copy_if_changed "$BUILD_BINARY" "$APP_SOURCE_BINARY_CACHE"; then
		mkdir -p "$(dirname "$APP_SOURCE_BINARY_CACHE")"
		cp "$BUILD_BINARY" "$APP_BINARY"
		STAGED_APP_DIRTY=1
		chmod +x "$APP_BINARY"
	fi

	if otool -L "$APP_BINARY" | grep -q 'Sparkle.framework' \
		&& ! otool -l "$APP_BINARY" | grep -q '@executable_path/../Frameworks'; then
		install_name_tool -add_rpath '@executable_path/../Frameworks' "$APP_BINARY"
		STAGED_APP_DIRTY=1
	fi
	sanitize_staged_app_rpaths

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

	stage_sparkle_framework

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
  <key>SUFeedURL</key>
  <string>$SPARKLE_APPCAST_URL</string>
  <key>SUEnableAutomaticChecks</key>
  <true/>
  <key>SUScheduledCheckInterval</key>
  <integer>86400</integer>
  <key>SUAutomaticallyUpdate</key>
  <true/>
  <key>SUAllowsAutomaticUpdates</key>
  <true/>
  <key>SUPublicEDKey</key>
  <string>$SPARKLE_PUBLIC_ED_KEY</string>
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
	local requested_identity
	requested_identity="${RSNAP_NATIVE_HOST_SIGN_IDENTITY:-$DEFAULT_SIGN_IDENTITY}"
	if [[ "$STAGED_APP_DIRTY" != "1" ]] && codesign --verify --deep --strict "$APP_BUNDLE" >/dev/null 2>&1; then
		return
	fi
	if resolve_signing_identity; then
		if [[ -f "$BUILD_ROOT/$EXECUTABLE_NAME-entitlement.plist" ]]; then
			codesign \
				--force \
				--deep \
				--options runtime \
				--sign "$RESOLVED_SIGN_IDENTITY" \
				--entitlements "$BUILD_ROOT/$EXECUTABLE_NAME-entitlement.plist" \
				"$APP_BUNDLE"
		else
			codesign \
				--force \
				--deep \
				--options runtime \
				--sign "$RESOLVED_SIGN_IDENTITY" \
				"$APP_BUNDLE"
		fi
		return
	fi

	echo "error: no valid macOS codesigning identity matching \"$requested_identity\" was found." >&2
	echo "error: import the real signing certificate or set RSNAP_NATIVE_HOST_SIGN_IDENTITY to a valid identity." >&2
	echo "error: Rsnap native host staging requires a stable codesigning identity." >&2
	exit 1
}

compute_stage_fingerprint() {
	local swift_toolchain
	swift_toolchain="$(
		{
			command -v swift
			swift --version
		} 2>/dev/null | tr '\n' ' '
	)"
	python3 - "$ROOT_DIR" "$RUST_PROFILE" "$SWIFT_CONFIGURATION" "$APP_VERSION" "$swift_toolchain" <<'PY'
import hashlib
import os
import sys

root, rust_profile, swift_configuration, app_version, swift_toolchain = sys.argv[1:]
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
# Toolchain changes can flip Swift compile-time availability gates, including Liquid Glass.
for value in (rust_profile, swift_configuration, app_version, swift_toolchain):
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
	collect_running_host_pids() {
		{
			pgrep -x "$EXECUTABLE_NAME" || true
			pgrep -f "$APP_BINARY" || true
		} | awk 'NF && !seen[$0]++'
	}
	while IFS= read -r pid; do
		[[ -n "$pid" ]] || continue
		kill "$pid" >/dev/null 2>&1 || true
	done <<<"$(collect_running_host_pids)"
	for ((attempt = 0; attempt < 20; attempt++)); do
		remaining_pids="$(collect_running_host_pids)"
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

canonicalize_app_bundle_name
if [[ "${RSNAP_NATIVE_HOST_FORCE_REBUILD:-0}" != "1" ]] && staged_bundle_is_current; then
	BUILD_ROOT=""
	BUILD_BINARY="$APP_BINARY"
else
	cargo build "${RUST_BUILD_ARGS[@]}"
	if [[ "${RSNAP_NATIVE_HOST_SWIFT_CLEAN:-0}" == "1" ]]; then
		swift package --package-path "$PACKAGE_DIR" clean
	fi
	BUILD_ROOT="$(RSNAP_HOST_FFI_LIB_DIR="$RUST_LIB_DIR" swift build --package-path "$PACKAGE_DIR" "${SWIFT_BUILD_FLAGS[@]}" --show-bin-path)"
	BUILD_BINARY="$BUILD_ROOT/$EXECUTABLE_NAME"
	# SwiftPM does not track the external Rust static library as a product input. Remove the
	# executable before building so Rust host-FFI changes are always relinked into the app bundle.
	rm -f "$BUILD_BINARY"
	RSNAP_HOST_FFI_LIB_DIR="$RUST_LIB_DIR" \
		swift build --package-path "$PACKAGE_DIR" "${SWIFT_BUILD_FLAGS[@]}" --product "$EXECUTABLE_NAME"

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
