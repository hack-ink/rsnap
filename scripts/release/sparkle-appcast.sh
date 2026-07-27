#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat >&2 <<'USAGE'
usage: scripts/release/sparkle-appcast.sh --archive ZIP --appcast XML --version VERSION --tag TAG

Creates a Sparkle appcast for the signed Rsnap macOS release archive.

Required environment:
  SPARKLE_PRIVATE_ED_KEY  private EdDSA key used by Sparkle sign_update

Optional environment:
  SPARKLE_SIGN_UPDATE     explicit path to Sparkle's sign_update tool
  SPARKLE_ARCHIVE_URL     canonical acg-box release URL, or a loopback smoke URL
  SPARKLE_RELEASE_NOTES_URL
                          canonical acg-box release URL, or a loopback smoke URL
USAGE
}

archive=""
appcast=""
version=""
tag=""

while [[ $# -gt 0 ]]; do
	case "$1" in
		--archive)
			archive="${2:-}"
			shift 2
			;;
		--appcast)
			appcast="${2:-}"
			shift 2
			;;
		--version)
			version="${2:-}"
			shift 2
			;;
		--tag)
			tag="${2:-}"
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

for required_value in archive appcast version tag; do
	if [[ -z "${!required_value}" ]]; then
		echo "error: missing --${required_value}" >&2
		usage
		exit 2
	fi
done

semver_pattern='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
if [[ ! "$version" =~ $semver_pattern ]]; then
	echo "error: version must be a stable SemVer value such as 1.2.3" >&2
	exit 1
fi
if [[ "$tag" != "v$version" ]]; then
	echo "error: release tag $tag does not match version $version" >&2
	exit 1
fi

if [[ ! -f "$archive" ]]; then
	echo "error: release archive not found: $archive" >&2
	exit 1
fi
if [[ ! -s "$archive" ]]; then
	echo "error: release archive is empty: $archive" >&2
	exit 1
fi

archive_name="$(basename "$archive")"
archive_name_pattern='^[A-Za-z0-9][A-Za-z0-9._-]*$'
if [[ ! "$archive_name" =~ $archive_name_pattern ]]; then
	echo "error: release archive name contains unsupported URL characters: $archive_name" >&2
	exit 1
fi

if [[ -z "${SPARKLE_PRIVATE_ED_KEY:-}" ]]; then
	echo "error: SPARKLE_PRIVATE_ED_KEY is required to sign the Sparkle update archive" >&2
	exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
sign_update="${SPARKLE_SIGN_UPDATE:-}"
if [[ -z "$sign_update" ]]; then
	sign_update="$(
		find "$repo_root/native/macos-host/.build/artifacts" \
			-type f \
			-path '*/bin/sign_update' \
			-print \
			-quit
	)"
fi

if [[ -z "$sign_update" || ! -x "$sign_update" ]]; then
	echo "error: Sparkle sign_update tool was not found or is not executable" >&2
	echo "error: run swift package resolve/build for native/macos-host first" >&2
	exit 1
fi

signature_output="$(
	printf '%s\n' "$SPARKLE_PRIVATE_ED_KEY" \
		| "$sign_update" --ed-key-file - "$archive"
)"

VERSION="$version" \
TAG="$tag" \
ARCHIVE_PATH="$archive" \
ARCHIVE_NAME="$archive_name" \
APPCAST="$appcast" \
SPARKLE_ARCHIVE_URL="${SPARKLE_ARCHIVE_URL:-}" \
SPARKLE_RELEASE_NOTES_URL="${SPARKLE_RELEASE_NOTES_URL:-}" \
SPARKLE_SIGNATURE_OUTPUT="$signature_output" \
python3 "$repo_root/scripts/release/write-sparkle-appcast.py"
