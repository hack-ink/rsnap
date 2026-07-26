#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat >&2 <<'USAGE'
usage: scripts/release/sparkle-appcast.sh --archive ZIP --appcast XML --version VERSION --tag TAG

Creates a Sparkle appcast for the signed Rsnap macOS release archive.

Required environment:
  RSNAP_SPARKLE_PRIVATE_ED_KEY
                          Rsnap private EdDSA key used by Sparkle sign_update

Optional environment:
  SPARKLE_SIGN_UPDATE     explicit path to Sparkle's sign_update tool
  SPARKLE_ARCHIVE_URL     explicit appcast download URL for the release archive
  SPARKLE_RELEASE_NOTES_URL
                          explicit appcast release notes URL
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

if [[ ! -f "$archive" ]]; then
	echo "error: release archive not found: $archive" >&2
	exit 1
fi

stable_version_re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
if [[ ! "$version" =~ $stable_version_re || "$tag" != "v$version" ]]; then
	echo "error: version and tag must match stable SemVer VERSION and vVERSION" >&2
	exit 1
fi

if [[ -z "${RSNAP_SPARKLE_PRIVATE_ED_KEY:-}" ]]; then
	echo "error: RSNAP_SPARKLE_PRIVATE_ED_KEY is required to sign the update archive" >&2
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

signature="$(
	printf '%s\n' "$RSNAP_SPARKLE_PRIVATE_ED_KEY" \
		| "$sign_update" --ed-key-file - -p "$archive"
)"
if [[ -z "$signature" || "$signature" == *$'\n'* || "$signature" == *$'\r'* ]]; then
	echo "error: Sparkle sign_update returned an invalid signature" >&2
	exit 1
fi
printf '%s\n' "$RSNAP_SPARKLE_PRIVATE_ED_KEY" \
	| "$sign_update" --verify --ed-key-file - "$archive" "$signature"
archive_length="$(wc -c <"$archive" | tr -d '[:space:]')"

VERSION="$version" \
	TAG="$tag" \
	ARCHIVE="$(basename "$archive")" \
	APPCAST="$appcast" \
	SPARKLE_ARCHIVE_URL="${SPARKLE_ARCHIVE_URL:-}" \
	SPARKLE_RELEASE_NOTES_URL="${SPARKLE_RELEASE_NOTES_URL:-}" \
	SPARKLE_SIGNATURE="$signature" \
	SPARKLE_ARCHIVE_LENGTH="$archive_length" \
	python3 - <<'PY'
import base64
import email.utils
import os
from pathlib import Path
import xml.etree.ElementTree as ET

version = os.environ["VERSION"]
tag = os.environ["TAG"]
archive = os.environ["ARCHIVE"]
appcast = os.environ["APPCAST"]
signature = os.environ["SPARKLE_SIGNATURE"].strip()
archive_length = os.environ["SPARKLE_ARCHIVE_LENGTH"]
archive_url = os.environ["SPARKLE_ARCHIVE_URL"].strip()
release_notes_url = os.environ["SPARKLE_RELEASE_NOTES_URL"].strip()

try:
	decoded_signature = base64.b64decode(signature, validate=True)
except ValueError as error:
	raise SystemExit(f"error: Sparkle signature is not valid base64: {error}") from error
if len(decoded_signature) != 64:
	raise SystemExit("error: Sparkle Ed25519 signature must decode to 64 bytes")
if not archive_length.isdecimal() or int(archive_length) <= 0:
	raise SystemExit("error: release archive length must be a positive integer")

download_url = archive_url or f"https://github.com/acg-box/rsnap/releases/download/{tag}/{archive}"
release_url = release_notes_url or f"https://github.com/acg-box/rsnap/releases/tag/{tag}"
pub_date = email.utils.formatdate(usegmt=True)

namespace = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", namespace)

rss = ET.Element("rss", {"version": "2.0"})
channel = ET.SubElement(rss, "channel")
ET.SubElement(channel, "title").text = "Rsnap Updates"
ET.SubElement(channel, "link").text = "https://github.com/acg-box/rsnap/releases"
ET.SubElement(channel, "description").text = "Rsnap macOS app updates."
ET.SubElement(channel, "language").text = "en"
item = ET.SubElement(channel, "item")
ET.SubElement(item, "title").text = f"Version {version}"
ET.SubElement(item, "link").text = release_url
ET.SubElement(item, f"{{{namespace}}}version").text = version
ET.SubElement(item, f"{{{namespace}}}shortVersionString").text = version
ET.SubElement(item, f"{{{namespace}}}minimumSystemVersion").text = "14.0.0"
ET.SubElement(item, f"{{{namespace}}}hardwareRequirements").text = "arm64"
ET.SubElement(item, f"{{{namespace}}}releaseNotesLink").text = release_url
ET.SubElement(item, "pubDate").text = pub_date
ET.SubElement(
	item,
	"enclosure",
	{
		"url": download_url,
		f"{{{namespace}}}edSignature": signature,
		"length": archive_length,
		"type": "application/octet-stream",
	},
)
ET.indent(rss, space="  ")
ET.ElementTree(rss).write(appcast, encoding="utf-8", xml_declaration=True)
PY
