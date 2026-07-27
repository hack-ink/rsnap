#!/usr/bin/env python3
"""Validate the complete Rsnap GitHub release artifact set."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import pathlib
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from typing import Any
from xml.etree import ElementTree


ARCHIVE_NAME = "rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME = "appcast.xml"
CHECKSUM_NAME = f"{ARCHIVE_NAME}.sha256"
APP_BUNDLE_NAME = "Rsnap.app"
BUNDLE_IDENTIFIER = "ink.hack.rsnap"
CANONICAL_REPOSITORY = "acg-box/rsnap"
SPARKLE_FEED_URL = (
    "https://github.com/acg-box/rsnap/releases/latest/download/appcast.xml"
)
SPARKLE_PUBLIC_ED_KEY = "X2EaTv6mCzkYxz75Hh+ldMkKlpzNlHRg5l7Kn9ke8Ow="
SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
SEMVER_PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
MAX_METADATA_BYTES = 4 * 1024 * 1024


class ValidationError(ValueError):
    """Report an invalid or inconsistent release artifact."""


def require(condition: bool, message: str) -> None:
    """Raise a release validation error when a condition is false."""
    if not condition:
        raise ValidationError(message)


def load_json(path: pathlib.Path) -> Any:
    """Load bounded JSON metadata."""
    require(path.is_file(), f"metadata file does not exist: {path}")
    require(
        path.stat().st_size <= MAX_METADATA_BYTES,
        f"metadata file is too large: {path}",
    )
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ValidationError(f"cannot read JSON metadata {path}: {error}") from error


def validate_identity(
    archive: pathlib.Path,
    appcast: pathlib.Path,
    checksum: pathlib.Path,
    version: str,
    tag: str,
    repository: str,
) -> None:
    """Validate fixed names and release identity inputs."""
    require(repository == CANONICAL_REPOSITORY, "repository must be acg-box/rsnap")
    require(SEMVER_PATTERN.fullmatch(version) is not None, "version must be X.Y.Z")
    require(tag == f"v{version}", "tag must be v followed by the release version")

    expected_names = (
        (archive, ARCHIVE_NAME),
        (appcast, APPCAST_NAME),
        (checksum, CHECKSUM_NAME),
    )
    for path, expected_name in expected_names:
        require(path.is_file(), f"release artifact does not exist: {path}")
        require(
            path.name == expected_name,
            f"release artifact must be named {expected_name}: {path}",
        )


def validate_archive(
    archive: pathlib.Path,
    version: str,
    public_key_b64: str,
) -> None:
    """Validate the app root and its release identity without extracting the ZIP."""
    try:
        with zipfile.ZipFile(archive) as bundle_zip:
            entries = bundle_zip.infolist()
            require(entries, "release archive is empty")

            names: set[str] = set()
            payload_names: list[str] = []
            for entry in entries:
                name = entry.filename
                require(name not in names, f"release archive has duplicate entry: {name}")
                names.add(name)
                require("\x00" not in name, "release archive has a NUL in an entry name")
                require("\\" not in name, f"release archive has a backslash path: {name}")
                path = pathlib.PurePosixPath(name)
                require(not path.is_absolute(), f"release archive has an absolute path: {name}")
                require(
                    ".." not in path.parts,
                    f"release archive has a parent path component: {name}",
                )

                # ditto can add AppleDouble metadata. It is not a second payload root.
                if path.parts and path.parts[0] == "__MACOSX":
                    continue
                require(path.parts, "release archive has an empty entry name")
                require(
                    path.parts[0] == APP_BUNDLE_NAME,
                    f"release archive has a second payload root: {name}",
                )
                payload_names.append(name)

            require(payload_names, "release archive does not contain Rsnap.app")
            info_name = f"{APP_BUNDLE_NAME}/Contents/Info.plist"
            require(info_name in names, "Rsnap.app does not contain Contents/Info.plist")
            info_entry = bundle_zip.getinfo(info_name)
            require(
                info_entry.file_size <= MAX_METADATA_BYTES,
                "Info.plist is too large",
            )
            try:
                info = plistlib.loads(bundle_zip.read(info_entry))
            except (OSError, plistlib.InvalidFileException) as error:
                raise ValidationError(f"cannot parse Info.plist: {error}") from error

            sparkle_prefix = (
                f"{APP_BUNDLE_NAME}/Contents/Frameworks/Sparkle.framework/"
            )
            require(
                any(
                    name.startswith(sparkle_prefix)
                    and not bundle_zip.getinfo(name).is_dir()
                    for name in payload_names
                ),
                "Rsnap.app does not contain Sparkle.framework",
            )
    except (OSError, zipfile.BadZipFile, KeyError) as error:
        raise ValidationError(f"cannot read release archive: {error}") from error

    require(isinstance(info, dict), "Info.plist root must be a dictionary")
    expected_values = {
        "CFBundleName": "Rsnap",
        "CFBundleDisplayName": "Rsnap",
        "CFBundleIdentifier": BUNDLE_IDENTIFIER,
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "SUFeedURL": SPARKLE_FEED_URL,
        "SUPublicEDKey": public_key_b64,
    }
    for key, expected in expected_values.items():
        require(
            info.get(key) == expected,
            f"Info.plist {key} must be {expected!r}",
        )


def decode_signature(signature_text: str) -> bytes:
    """Decode and validate a Sparkle Ed25519 signature."""
    try:
        signature = base64.b64decode(signature_text, validate=True)
    except (ValueError, binascii.Error) as error:
        raise ValidationError("appcast Ed25519 signature is not valid base64") from error
    require(len(signature) == 64, "appcast Ed25519 signature must be 64 bytes")
    return signature


def verify_ed25519_signature(
    archive: pathlib.Path,
    signature: bytes,
    public_key_b64: str,
) -> None:
    """Verify an Ed25519 signature with the OpenSSL platform primitive."""
    openssl = shutil.which("openssl")
    require(openssl is not None, "openssl is required to verify the appcast signature")
    try:
        public_key = base64.b64decode(public_key_b64, validate=True)
    except (ValueError, binascii.Error) as error:
        raise ValidationError("checked-in Sparkle public key is not valid base64") from error
    require(len(public_key) == 32, "checked-in Sparkle public key must be 32 bytes")

    # RFC 8410 SubjectPublicKeyInfo for an Ed25519 raw public key.
    public_key_der = bytes.fromhex("302a300506032b6570032100") + public_key
    public_key_pem = (
        b"-----BEGIN PUBLIC KEY-----\n"
        + base64.encodebytes(public_key_der)
        + b"-----END PUBLIC KEY-----\n"
    )

    with tempfile.TemporaryDirectory(prefix="rsnap-release-signature-") as temp_dir:
        temp_path = pathlib.Path(temp_dir)
        public_key_path = temp_path / "sparkle-public.pem"
        signature_path = temp_path / "sparkle-signature.bin"
        public_key_path.write_bytes(public_key_pem)
        signature_path.write_bytes(signature)
        result = subprocess.run(
            [
                openssl,
                "pkeyutl",
                "-verify",
                "-pubin",
                "-inkey",
                str(public_key_path),
                "-rawin",
                "-in",
                str(archive),
                "-sigfile",
                str(signature_path),
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    require(result.returncode == 0, "appcast Ed25519 signature verification failed")


def required_element_text(
    parent: ElementTree.Element,
    name: str,
    description: str,
) -> str:
    """Read one required child element as non-empty text."""
    elements = parent.findall(name)
    require(len(elements) == 1, f"appcast must contain one {description}")
    value = elements[0].text
    require(value is not None and value.strip() != "", f"appcast {description} is empty")
    return value.strip()


def validate_appcast(
    appcast: pathlib.Path,
    archive: pathlib.Path,
    version: str,
    tag: str,
    verify_signature: bool,
    public_key_b64: str,
) -> None:
    """Validate the stable Sparkle item and its archive enclosure."""
    require(appcast.stat().st_size <= MAX_METADATA_BYTES, "appcast.xml is too large")
    try:
        appcast_bytes = appcast.read_bytes()
    except OSError as error:
        raise ValidationError(f"cannot read appcast.xml: {error}") from error
    upper_bytes = appcast_bytes.upper()
    require(b"<!DOCTYPE" not in upper_bytes, "appcast.xml must not contain a DOCTYPE")
    require(b"<!ENTITY" not in upper_bytes, "appcast.xml must not contain entities")
    try:
        root = ElementTree.fromstring(appcast_bytes)
    except ElementTree.ParseError as error:
        raise ValidationError(f"cannot parse appcast.xml: {error}") from error

    require(root.tag == "rss", "appcast root must be rss")
    channels = root.findall("channel")
    require(len(channels) == 1, "appcast must contain one channel")
    items = channels[0].findall("item")
    require(len(items) == 1, "appcast must contain one stable release item")
    item = items[0]

    channel_name = f"{{{SPARKLE_NAMESPACE}}}channel"
    require(item.find(channel_name) is None, "appcast release item must use the stable channel")
    require(
        required_element_text(
            item,
            f"{{{SPARKLE_NAMESPACE}}}version",
            "sparkle:version",
        )
        == version,
        "appcast sparkle:version does not match the release version",
    )
    require(
        required_element_text(
            item,
            f"{{{SPARKLE_NAMESPACE}}}shortVersionString",
            "sparkle:shortVersionString",
        )
        == version,
        "appcast short version does not match the release version",
    )

    release_url = f"https://github.com/{CANONICAL_REPOSITORY}/releases/tag/{tag}"
    require(
        required_element_text(item, "link", "release link") == release_url,
        "appcast release link is not canonical",
    )
    require(
        required_element_text(
            item,
            f"{{{SPARKLE_NAMESPACE}}}releaseNotesLink",
            "release notes link",
        )
        == release_url,
        "appcast release notes link is not canonical",
    )

    enclosures = item.findall("enclosure")
    require(len(enclosures) == 1, "appcast must contain one enclosure")
    enclosure = enclosures[0]
    archive_url = (
        f"https://github.com/{CANONICAL_REPOSITORY}/releases/download/"
        f"{tag}/{ARCHIVE_NAME}"
    )
    require(enclosure.get("url") == archive_url, "appcast archive URL is not canonical")
    require(
        enclosure.get("length") == str(archive.stat().st_size),
        "appcast archive length does not match the ZIP",
    )
    signature_text = enclosure.get(f"{{{SPARKLE_NAMESPACE}}}edSignature")
    require(signature_text is not None, "appcast enclosure has no Ed25519 signature")
    signature = decode_signature(signature_text)
    if verify_signature:
        verify_ed25519_signature(archive, signature, public_key_b64)


def validate_checksum(checksum: pathlib.Path, archive: pathlib.Path) -> None:
    """Validate the canonical SHA-256 checksum file byte for byte."""
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    expected = f"{digest}  {ARCHIVE_NAME}\n".encode()
    try:
        actual = checksum.read_bytes()
    except OSError as error:
        raise ValidationError(f"cannot read checksum file: {error}") from error
    require(
        actual == expected,
        f"{CHECKSUM_NAME} must contain the exact SHA-256 line for {ARCHIVE_NAME}",
    )


def validate_release_metadata(
    metadata: Any,
    tag: str,
    repository: str,
    release_state: str | None,
) -> tuple[int, str]:
    """Validate GitHub release metadata and return its ID and download tag."""
    require(isinstance(metadata, dict), "GitHub release metadata must be an object")
    release_id = metadata.get("id")
    require(
        isinstance(release_id, int) and release_id > 0,
        "GitHub release ID must be a positive integer",
    )
    require(metadata.get("tag_name") == tag, "GitHub release tag does not match")
    require(metadata.get("prerelease") is False, "GitHub release must not be a prerelease")

    expected_api_url = f"https://api.github.com/repos/{repository}/releases/{release_id}"
    require(
        metadata.get("url") == expected_api_url,
        "GitHub release API URL is not canonical",
    )

    download_tag = tag
    if release_state is not None:
        expected_draft = release_state == "draft"
        require(
            metadata.get("draft") is expected_draft,
            f"GitHub release must be {release_state}",
        )
    if release_state == "draft":
        draft_url_prefix = f"https://github.com/{repository}/releases/tag/"
        html_url = metadata.get("html_url")
        require(
            isinstance(html_url, str) and html_url.startswith(draft_url_prefix),
            "GitHub draft release URL is not canonical",
        )
        download_tag = html_url.removeprefix(draft_url_prefix)
        require(
            re.fullmatch(r"untagged-[A-Za-z0-9][A-Za-z0-9._-]*", download_tag)
            is not None,
            "GitHub draft release URL has an invalid untagged token",
        )
    else:
        expected_html_url = f"https://github.com/{repository}/releases/tag/{tag}"
        require(
            metadata.get("html_url") == expected_html_url,
            "GitHub release URL is not canonical",
        )
    return release_id, download_tag


def validate_asset_metadata(
    metadata: Any,
    release_id: int,
    download_tag: str,
    archive: pathlib.Path,
    appcast: pathlib.Path,
    checksum: pathlib.Path,
    repository: str,
) -> None:
    """Validate the exact GitHub release asset metadata set."""
    require(isinstance(metadata, list), "GitHub asset metadata must be an array")
    local_paths = {
        ARCHIVE_NAME: archive,
        APPCAST_NAME: appcast,
        CHECKSUM_NAME: checksum,
    }
    require(
        len(metadata) == len(local_paths),
        "GitHub release must contain exactly three assets",
    )

    seen_names: set[str] = set()
    for asset in metadata:
        require(isinstance(asset, dict), "GitHub asset metadata entry must be an object")
        asset_id = asset.get("id")
        name = asset.get("name")
        require(
            isinstance(asset_id, int) and asset_id > 0,
            "GitHub asset ID must be a positive integer",
        )
        require(isinstance(name, str), "GitHub asset name must be a string")
        require(name not in seen_names, f"GitHub release has duplicate asset: {name}")
        seen_names.add(name)
        require(name in local_paths, f"GitHub release has unexpected asset: {name}")
        require(
            asset.get("size") == local_paths[name].stat().st_size,
            f"GitHub asset size does not match local file: {name}",
        )
        expected_api_url = (
            f"https://api.github.com/repos/{repository}/releases/assets/{asset_id}"
        )
        expected_download_url = (
            f"https://github.com/{repository}/releases/download/{download_tag}/{name}"
        )
        require(
            asset.get("url") == expected_api_url,
            f"GitHub asset API URL is not canonical: {name}",
        )
        require(
            asset.get("browser_download_url") == expected_download_url,
            f"GitHub asset download URL is not canonical: {name}",
        )
        require(
            asset.get("state") == "uploaded",
            f"GitHub asset is not fully uploaded: {name}",
        )
    require(seen_names == set(local_paths), "GitHub release asset set is incomplete")


def validate_artifacts(
    *,
    archive: pathlib.Path,
    appcast: pathlib.Path,
    checksum: pathlib.Path,
    version: str,
    tag: str,
    repository: str,
    verify_signature: bool = False,
    public_key_b64: str = SPARKLE_PUBLIC_ED_KEY,
    release_json: pathlib.Path | None = None,
    assets_json: pathlib.Path | None = None,
    release_state: str | None = None,
) -> None:
    """Validate local artifacts and optional GitHub metadata."""
    validate_identity(archive, appcast, checksum, version, tag, repository)
    validate_archive(archive, version, public_key_b64)
    validate_appcast(
        appcast,
        archive,
        version,
        tag,
        verify_signature,
        public_key_b64,
    )
    validate_checksum(checksum, archive)

    if assets_json is not None:
        require(release_json is not None, "--assets-json requires --release-json")
    if release_state is not None:
        require(release_json is not None, "release metadata options require --release-json")
    if release_json is not None:
        release_id, download_tag = validate_release_metadata(
            load_json(release_json),
            tag,
            repository,
            release_state,
        )
        if assets_json is not None:
            validate_asset_metadata(
                load_json(assets_json),
                release_id,
                download_tag,
                archive,
                appcast,
                checksum,
                repository,
            )


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse the release validator command line."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=pathlib.Path)
    parser.add_argument("--appcast", required=True, type=pathlib.Path)
    parser.add_argument("--checksum", required=True, type=pathlib.Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument(
        "--verify-appcast-signature",
        action="store_true",
        help="verify the appcast Ed25519 signature with OpenSSL",
    )
    parser.add_argument("--release-json", type=pathlib.Path)
    parser.add_argument("--assets-json", type=pathlib.Path)
    parser.add_argument("--release-state", choices=("draft", "published"))
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the release artifact validator."""
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        validate_artifacts(
            archive=args.archive,
            appcast=args.appcast,
            checksum=args.checksum,
            version=args.version,
            tag=args.tag,
            repository=args.repository,
            verify_signature=args.verify_appcast_signature,
            release_json=args.release_json,
            assets_json=args.assets_json,
            release_state=args.release_state,
        )
    except ValidationError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"validated Rsnap release artifacts for {args.tag}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
