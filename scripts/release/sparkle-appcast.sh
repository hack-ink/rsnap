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

signature_fragment="$(
	printf '%s\n' "$SPARKLE_PRIVATE_ED_KEY" \
		| "$sign_update" --ed-key-file - "$archive"
)"
if [[ "$signature_fragment" != *"sparkle:edSignature="* || "$signature_fragment" != *"length="* ]]; then
	echo "error: unexpected Sparkle signature fragment: $signature_fragment" >&2
	exit 1
fi

VERSION="$version" \
	TAG="$tag" \
	ARCHIVE="$(basename "$archive")" \
	APPCAST="$appcast" \
	SPARKLE_ARCHIVE_URL="${SPARKLE_ARCHIVE_URL:-}" \
	SPARKLE_RELEASE_NOTES_URL="${SPARKLE_RELEASE_NOTES_URL:-}" \
	SPARKLE_SIGNATURE_FRAGMENT="$signature_fragment" \
	python3 - <<'PY'
import email.utils
import os
from pathlib import Path
from textwrap import dedent
from xml.sax.saxutils import escape

version = os.environ["VERSION"]
tag = os.environ["TAG"]
archive = os.environ["ARCHIVE"]
appcast = os.environ["APPCAST"]
signature_fragment = os.environ["SPARKLE_SIGNATURE_FRAGMENT"].strip()
archive_url = os.environ["SPARKLE_ARCHIVE_URL"].strip()
release_notes_url = os.environ["SPARKLE_RELEASE_NOTES_URL"].strip()

download_url = archive_url or f"https://github.com/hack-ink/rsnap/releases/download/{tag}/{archive}"
release_url = release_notes_url or f"https://github.com/hack-ink/rsnap/releases/tag/{tag}"
pub_date = email.utils.formatdate(usegmt=True)

xml = dedent(
	f"""\
	<?xml version="1.0" encoding="UTF-8"?>
	<rss version="2.0"
	  xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
	  <channel>
	    <title>Rsnap Updates</title>
	    <link>https://github.com/hack-ink/rsnap/releases</link>
	    <description>Rsnap macOS app updates.</description>
	    <language>en</language>
	    <item>
	      <title>Version {escape(version)}</title>
	      <link>{escape(release_url)}</link>
	      <sparkle:version>{escape(version)}</sparkle:version>
	      <sparkle:shortVersionString>{escape(version)}</sparkle:shortVersionString>
	      <sparkle:minimumSystemVersion>14.0</sparkle:minimumSystemVersion>
	      <sparkle:hardwareRequirements>arm64</sparkle:hardwareRequirements>
	      <sparkle:releaseNotesLink>{escape(release_url)}</sparkle:releaseNotesLink>
	      <pubDate>{escape(pub_date)}</pubDate>
	      <enclosure
	        url="{escape(download_url)}"
	        {signature_fragment}
	        type="application/octet-stream" />
	    </item>
	  </channel>
	</rss>
	"""
)
Path(appcast).write_text(xml, encoding="utf-8")
PY
