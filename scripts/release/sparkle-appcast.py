#!/usr/bin/env python3
"""Create the signed Sparkle appcast for an Rsnap release archive."""

from __future__ import annotations

import argparse
import base64
import binascii
import email.utils
import os
import re
import subprocess
import sys
import tempfile
import urllib.parse
import xml.etree.ElementTree as ElementTree
from pathlib import Path


CANONICAL_REPOSITORY = "acg-box/rsnap"
SEMVER_PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
SIGNATURE_PATTERN = re.compile(
    r'sparkle:edSignature="([A-Za-z0-9+/]+={0,2})" length="([1-9][0-9]*)"'
)
SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"


class AppcastError(RuntimeError):
    """Report invalid appcast input or Sparkle signer output."""


def require(condition: bool, message: str) -> None:
    """Raise an appcast error when a required condition is false."""
    if not condition:
        raise AppcastError(message)


def checked_url(value: str, canonical: str, label: str) -> str:
    """Accept the canonical release URL or a loopback smoke-test URL."""
    if not value or value == canonical:
        return canonical

    parsed = urllib.parse.urlsplit(value)
    is_loopback = (
        parsed.scheme in {"http", "https"}
        and parsed.hostname in {"127.0.0.1", "::1", "localhost"}
        and parsed.username is None
        and parsed.password is None
        and not parsed.fragment
    )
    require(is_loopback, f"{label} must use the canonical acg-box/rsnap release URL")
    return value


def resolve_sign_update(repo_root: Path) -> Path:
    """Resolve Sparkle's official sign_update executable."""
    override = os.environ.get("SPARKLE_SIGN_UPDATE", "").strip()
    if override:
        sign_update = Path(override)
        require(
            sign_update.is_file() and os.access(sign_update, os.X_OK),
            f"Sparkle sign_update is not executable: {sign_update}",
        )
        return sign_update

    candidate = (
        repo_root
        / "native/macos-host/.build/artifacts/sparkle/Sparkle/bin/sign_update"
    )
    if candidate.is_file() and os.access(candidate, os.X_OK):
        return candidate
    raise AppcastError(
        "Sparkle sign_update was not found; resolve or build native/macos-host first"
    )


def sign_archive(sign_update: Path, archive: Path) -> tuple[str, int]:
    """Sign the archive and validate Sparkle's signature record."""
    require(
        os.environ.get("SPARKLE_PRIVATE_ED_KEY", "").strip() == "",
        "generic SPARKLE_PRIVATE_ED_KEY is forbidden for Rsnap releases",
    )
    private_key = os.environ.get("RSNAP_SPARKLE_PRIVATE_ED_KEY", "")
    require(private_key.strip() != "", "RSNAP_SPARKLE_PRIVATE_ED_KEY is required")

    result = subprocess.run(
        [str(sign_update), "--ed-key-file", "-", str(archive)],
        input=private_key.rstrip("\r\n") + "\n",
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or "unknown sign_update error"
        raise AppcastError(f"Sparkle sign_update failed: {detail}")

    match = SIGNATURE_PATTERN.fullmatch(result.stdout.strip())
    require(match is not None, "Sparkle sign_update returned an invalid signature record")
    signature, length_text = match.groups()
    try:
        signature_bytes = base64.b64decode(signature, validate=True)
    except (ValueError, binascii.Error) as error:
        raise AppcastError("Sparkle returned a malformed EdDSA signature") from error
    require(len(signature_bytes) == 64, "Sparkle returned an invalid EdDSA signature")

    signed_length = int(length_text)
    require(
        signed_length == archive.stat().st_size,
        "Sparkle signed length does not match the archive",
    )
    return signature, signed_length


def write_appcast(
    appcast: Path,
    *,
    version: str,
    release_url: str,
    archive_url: str,
    archive_length: int,
    signature: str,
) -> None:
    """Atomically write one stable Sparkle release item."""
    ElementTree.register_namespace("sparkle", SPARKLE_NAMESPACE)
    rss = ElementTree.Element("rss", {"version": "2.0"})
    channel = ElementTree.SubElement(rss, "channel")
    ElementTree.SubElement(channel, "title").text = "Rsnap Updates"
    ElementTree.SubElement(channel, "link").text = (
        f"https://github.com/{CANONICAL_REPOSITORY}/releases"
    )
    ElementTree.SubElement(channel, "description").text = "Rsnap macOS app updates."
    ElementTree.SubElement(channel, "language").text = "en"

    item = ElementTree.SubElement(channel, "item")
    ElementTree.SubElement(item, "title").text = f"Version {version}"
    ElementTree.SubElement(item, "link").text = release_url
    ElementTree.SubElement(item, f"{{{SPARKLE_NAMESPACE}}}version").text = version
    ElementTree.SubElement(
        item, f"{{{SPARKLE_NAMESPACE}}}shortVersionString"
    ).text = version
    ElementTree.SubElement(
        item, f"{{{SPARKLE_NAMESPACE}}}minimumSystemVersion"
    ).text = "14.0"
    ElementTree.SubElement(
        item, f"{{{SPARKLE_NAMESPACE}}}hardwareRequirements"
    ).text = "arm64"
    ElementTree.SubElement(
        item, f"{{{SPARKLE_NAMESPACE}}}releaseNotesLink"
    ).text = release_url
    ElementTree.SubElement(item, "pubDate").text = email.utils.formatdate(usegmt=True)
    ElementTree.SubElement(
        item,
        "enclosure",
        {
            "url": archive_url,
            f"{{{SPARKLE_NAMESPACE}}}edSignature": signature,
            "length": str(archive_length),
            "type": "application/octet-stream",
        },
    )

    tree = ElementTree.ElementTree(rss)
    ElementTree.indent(tree, space="  ")
    appcast.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb",
            dir=appcast.parent,
            prefix=f".{appcast.name}.",
            delete=False,
        ) as temporary_file:
            temporary_path = Path(temporary_file.name)
            tree.write(temporary_file, encoding="utf-8", xml_declaration=True)
        os.replace(temporary_path, appcast)
    finally:
        if temporary_path is not None and temporary_path.exists():
            temporary_path.unlink()


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--appcast", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Validate inputs, sign the archive, and write the appcast."""
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        require(
            SEMVER_PATTERN.fullmatch(args.version) is not None,
            "version must be stable X.Y.Z SemVer",
        )
        require(args.tag == f"v{args.version}", "tag does not match version")
        require(args.archive.is_file(), f"archive not found: {args.archive}")
        require(args.archive.stat().st_size > 0, "archive is empty")
        require(
            re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", args.archive.name)
            is not None,
            "archive name contains unsupported URL characters",
        )
        require(
            args.appcast.resolve() != args.archive.resolve(),
            "appcast output must not replace the archive",
        )

        canonical_release_url = (
            f"https://github.com/{CANONICAL_REPOSITORY}/releases/tag/{args.tag}"
        )
        canonical_archive_url = (
            f"https://github.com/{CANONICAL_REPOSITORY}/releases/download/"
            f"{args.tag}/{args.archive.name}"
        )
        release_url = checked_url(
            os.environ.get("SPARKLE_RELEASE_NOTES_URL", "").strip(),
            canonical_release_url,
            "SPARKLE_RELEASE_NOTES_URL",
        )
        archive_url = checked_url(
            os.environ.get("SPARKLE_ARCHIVE_URL", "").strip(),
            canonical_archive_url,
            "SPARKLE_ARCHIVE_URL",
        )
        repo_root = Path(__file__).resolve().parents[2]
        signature, archive_length = sign_archive(
            resolve_sign_update(repo_root),
            args.archive,
        )
        write_appcast(
            args.appcast,
            version=args.version,
            release_url=release_url,
            archive_url=archive_url,
            archive_length=archive_length,
            signature=signature,
        )
    except (AppcastError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
