#!/usr/bin/env python3
"""Validate Rsnap release app, archive, appcast, checksum, and release metadata."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path
from typing import Any


CANONICAL_REPOSITORY = "acg-box/rsnap"
APP_NAME = "Rsnap.app"
APP_EXECUTABLE = "RsnapNativeHost"
APP_BUNDLE_ID = "ink.hack.rsnap"
ARCHIVE_NAME = "rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME = "appcast.xml"
CHECKSUM_NAME = f"{ARCHIVE_NAME}.sha256"
SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
RSNAP_SPARKLE_PUBLIC_KEY = "X2EaTv6mCzkYxz75Hh+ldMkKlpzNlHRg5l7Kn9ke8Ow="
SEMVER_COMPONENT = r"(?:0|[1-9][0-9]*)"
STABLE_VERSION_RE = re.compile(
	rf"^{SEMVER_COMPONENT}\.{SEMVER_COMPONENT}\.{SEMVER_COMPONENT}$"
)


class ValidationError(RuntimeError):
	"""A release artifact contract failed."""


def parse_args() -> argparse.Namespace:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--version", required=True)
	parser.add_argument("--sparkle-version", required=True)
	parser.add_argument("--tag", required=True)
	parser.add_argument("--repository", required=True)
	parser.add_argument("--app", type=Path)
	parser.add_argument("--archive", type=Path)
	parser.add_argument("--appcast", type=Path)
	parser.add_argument("--checksum", type=Path)
	parser.add_argument("--release-json", type=Path)
	parser.add_argument("--assets-json", type=Path)
	parser.add_argument("--release-state", choices=("draft", "published"))
	parser.add_argument("--verify-appcast-signature", action="store_true")
	return parser.parse_args()


def fail(message: str) -> None:
	raise ValidationError(message)


def read_plist_bytes(data: bytes, source: str) -> dict[str, Any]:
	try:
		plist = plistlib.loads(data)
	except plistlib.InvalidFileException as error:
		fail(f"invalid plist at {source}: {error}")
	if not isinstance(plist, dict):
		fail(f"plist at {source} is not a dictionary")
	return plist


def read_plist_file(path: Path) -> dict[str, Any]:
	try:
		return read_plist_bytes(path.read_bytes(), str(path))
	except OSError as error:
		fail(f"cannot read {path}: {error}")
	return {}


def validate_public_key(value: object) -> None:
	if value != RSNAP_SPARKLE_PUBLIC_KEY:
		fail("SUPublicEDKey does not match the checked-in Rsnap update key")
	try:
		decoded = base64.b64decode(str(value), validate=True)
	except (ValueError, binascii.Error) as error:
		fail(f"SUPublicEDKey is not valid base64: {error}")
	if len(decoded) != 32:
		fail("SUPublicEDKey must decode to a 32-byte Ed25519 public key")


def validate_main_plist(plist: dict[str, Any], *, version: str, repository: str) -> None:
	expected_feed = f"https://github.com/{repository}/releases/latest/download/{APPCAST_NAME}"
	expected_values = {
		"CFBundleName": "Rsnap",
		"CFBundleDisplayName": "Rsnap",
		"CFBundleIdentifier": APP_BUNDLE_ID,
		"CFBundleShortVersionString": version,
		"CFBundleVersion": version,
		"LSMinimumSystemVersion": "14.0",
		"SUFeedURL": expected_feed,
	}
	for key, expected in expected_values.items():
		actual = plist.get(key)
		if actual != expected:
			fail(f"{key} must be {expected!r}, got {actual!r}")
	validate_public_key(plist.get("SUPublicEDKey"))


def validate_sparkle_plist(plist: dict[str, Any], *, sparkle_version: str) -> None:
	if plist.get("CFBundleIdentifier") != "org.sparkle-project.Sparkle":
		fail("embedded framework is not org.sparkle-project.Sparkle")
	actual_version = plist.get("CFBundleShortVersionString")
	if actual_version != sparkle_version:
		fail(
			f"embedded Sparkle version {actual_version!r} does not match {sparkle_version}"
		)


def validate_app(path: Path, *, version: str, sparkle_version: str, repository: str) -> None:
	if path.name != APP_NAME or not path.is_dir():
		fail(f"app bundle must be an existing {APP_NAME}: {path}")
	main_executable = path / "Contents/MacOS" / APP_EXECUTABLE
	if not main_executable.is_file():
		fail(f"main executable is missing: {main_executable}")
	main_plist = read_plist_file(path / "Contents/Info.plist")
	validate_main_plist(main_plist, version=version, repository=repository)

	framework = path / "Contents/Frameworks/Sparkle.framework"
	current_link = framework / "Versions/Current"
	if not current_link.is_symlink() or current_link.readlink() != Path("B"):
		fail("Sparkle Versions/Current must point to B")
	version_root = framework / "Versions/B"
	for relative_path in (
		"Sparkle",
		"Autoupdate",
		"Updater.app",
		"XPCServices/Installer.xpc",
		"XPCServices/Downloader.xpc",
	):
		if not (version_root / relative_path).exists():
			fail(f"embedded Sparkle code is missing: {relative_path}")
	framework_plist = read_plist_file(version_root / "Resources/Info.plist")
	validate_sparkle_plist(framework_plist, sparkle_version=sparkle_version)


def zip_read(archive: zipfile.ZipFile, member: str) -> bytes:
	try:
		return archive.read(member)
	except KeyError:
		fail(f"release archive is missing {member}")
	return b""


def validate_archive_bundle(
	path: Path,
	*,
	version: str,
	sparkle_version: str,
	repository: str,
) -> None:
	if path.name != ARCHIVE_NAME or not path.is_file():
		fail(f"release archive must be an existing {ARCHIVE_NAME}: {path}")
	try:
		with zipfile.ZipFile(path) as archive:
			names = set(archive.namelist())
			archive_roots = {
				name.split("/", maxsplit=1)[0]
				for name in names
				if name and not name.startswith("__MACOSX/")
			}
			if archive_roots != {APP_NAME}:
				fail(
					f"release archive root must be only {APP_NAME}: {sorted(archive_roots)}"
				)
			main_plist = read_plist_bytes(
				zip_read(archive, f"{APP_NAME}/Contents/Info.plist"),
				f"{path}:{APP_NAME}/Contents/Info.plist",
			)
			validate_main_plist(main_plist, version=version, repository=repository)
			zip_read(archive, f"{APP_NAME}/Contents/MacOS/{APP_EXECUTABLE}")
			for relative_path in (
				"Sparkle",
				"Autoupdate",
				"Updater.app/Contents/Info.plist",
				"XPCServices/Installer.xpc/Contents/Info.plist",
				"XPCServices/Downloader.xpc/Contents/Info.plist",
			):
				zip_read(
					archive,
					f"{APP_NAME}/Contents/Frameworks/Sparkle.framework/Versions/B/"
					f"{relative_path}",
				)
			framework_plist = read_plist_bytes(
				zip_read(
					archive,
					f"{APP_NAME}/Contents/Frameworks/Sparkle.framework/Versions/B/"
					"Resources/Info.plist",
				),
				f"{path}:Sparkle.framework/Versions/B/Resources/Info.plist",
			)
			validate_sparkle_plist(framework_plist, sparkle_version=sparkle_version)
	except zipfile.BadZipFile as error:
		fail(f"invalid release ZIP: {error}")


def one_element(parent: ET.Element, path: str) -> ET.Element:
	elements = parent.findall(path)
	if len(elements) != 1:
		fail(f"appcast must contain exactly one {path}, found {len(elements)}")
	return elements[0]


def verify_appcast_signature(archive: Path, signature: bytes) -> None:
	openssl_name = os.environ.get("RSNAP_OPENSSL_BIN", "openssl")
	openssl_bin = shutil.which(openssl_name)
	if openssl_bin is None:
		fail(f"OpenSSL is required for appcast signature verification: {openssl_name}")
	public_key = base64.b64decode(RSNAP_SPARKLE_PUBLIC_KEY, validate=True)
	# SubjectPublicKeyInfo prefix for a raw RFC 8410 Ed25519 public key.
	public_key_der = bytes.fromhex("302a300506032b6570032100") + public_key
	with tempfile.TemporaryDirectory(prefix="rsnap-appcast-signature-") as temp_dir:
		public_key_path = Path(temp_dir) / "public.der"
		signature_path = Path(temp_dir) / "signature.bin"
		public_key_path.write_bytes(public_key_der)
		signature_path.write_bytes(signature)
		result = subprocess.run(
			[
				openssl_bin,
				"pkeyutl",
				"-verify",
				"-pubin",
				"-inkey",
				str(public_key_path),
				"-keyform",
				"DER",
				"-rawin",
				"-in",
				str(archive),
				"-sigfile",
				str(signature_path),
			],
			capture_output=True,
			text=True,
		)
	if result.returncode != 0:
		detail = result.stderr.strip() or result.stdout.strip() or "signature mismatch"
		fail(f"appcast Ed25519 signature verification failed: {detail}")


def validate_appcast(
	path: Path,
	*,
	archive: Path,
	version: str,
	tag: str,
	repository: str,
	verify_signature: bool,
) -> None:
	if path.name != APPCAST_NAME or not path.is_file():
		fail(f"appcast must be an existing {APPCAST_NAME}: {path}")
	try:
		root = ET.parse(path).getroot()
	except (OSError, ET.ParseError) as error:
		fail(f"invalid appcast XML: {error}")
	if root.tag != "rss" or root.get("version") != "2.0":
		fail("appcast root must be RSS 2.0")
	item = one_element(root, "./channel/item")
	expected_release_url = f"https://github.com/{repository}/releases/tag/{tag}"
	expected_values = {
		"link": expected_release_url,
		f"{{{SPARKLE_NAMESPACE}}}version": version,
		f"{{{SPARKLE_NAMESPACE}}}shortVersionString": version,
		f"{{{SPARKLE_NAMESPACE}}}minimumSystemVersion": "14.0.0",
		f"{{{SPARKLE_NAMESPACE}}}hardwareRequirements": "arm64",
		f"{{{SPARKLE_NAMESPACE}}}releaseNotesLink": expected_release_url,
	}
	for element_name, expected in expected_values.items():
		element = one_element(item, element_name)
		if element.text != expected:
			fail(f"appcast {element_name} must be {expected!r}, got {element.text!r}")

	enclosure = one_element(item, "enclosure")
	expected_archive_url = (
		f"https://github.com/{repository}/releases/download/{tag}/{ARCHIVE_NAME}"
	)
	if enclosure.get("url") != expected_archive_url:
		fail(f"appcast enclosure URL must be {expected_archive_url}")
	if enclosure.get("type") != "application/octet-stream":
		fail("appcast enclosure type must be application/octet-stream")
	actual_length = archive.stat().st_size
	if enclosure.get("length") != str(actual_length):
		fail(
			f"appcast enclosure length must be {actual_length}, "
			f"got {enclosure.get('length')!r}"
		)
	signature = enclosure.get(f"{{{SPARKLE_NAMESPACE}}}edSignature")
	if not signature:
		fail("appcast enclosure is missing sparkle:edSignature")
	try:
		decoded_signature = base64.b64decode(signature, validate=True)
	except (ValueError, binascii.Error) as error:
		fail(f"appcast EdDSA signature is not valid base64: {error}")
	if len(decoded_signature) != 64:
		fail("appcast Ed25519 signature must decode to 64 bytes")
	if verify_signature:
		verify_appcast_signature(archive, decoded_signature)


def sha256(path: Path) -> str:
	digest = hashlib.sha256()
	try:
		with path.open("rb") as handle:
			for chunk in iter(lambda: handle.read(1 << 20), b""):
				digest.update(chunk)
	except OSError as error:
		fail(f"cannot hash {path}: {error}")
	return digest.hexdigest()


def validate_checksum(path: Path, *, archive: Path) -> None:
	if path.name != CHECKSUM_NAME or not path.is_file():
		fail(f"checksum must be an existing {CHECKSUM_NAME}: {path}")
	try:
		lines = [line for line in path.read_text(encoding="utf-8").splitlines() if line]
	except OSError as error:
		fail(f"cannot read checksum: {error}")
	if len(lines) != 1:
		fail("checksum file must contain exactly one non-empty line")
	match = re.fullmatch(rf"([0-9a-f]{{64}})  {re.escape(ARCHIVE_NAME)}", lines[0])
	if match is None:
		fail("checksum file must use lowercase SHA-256 and the canonical archive name")
	expected_digest = sha256(archive)
	if match.group(1) != expected_digest:
		fail("checksum does not match the final release archive")


def read_json(path: Path) -> Any:
	try:
		return json.loads(path.read_text(encoding="utf-8"))
	except (OSError, json.JSONDecodeError) as error:
		fail(f"cannot read JSON from {path}: {error}")
	return None


def validate_release_metadata(
	release_path: Path,
	assets_path: Path,
	*,
	tag: str,
	repository: str,
	release_state: str,
	local_assets: dict[str, Path],
) -> None:
	release = read_json(release_path)
	if not isinstance(release, dict):
		fail("release API response must be an object")
	if release.get("tag_name") != tag:
		fail("release tag does not match the workflow tag")
	expected_draft = release_state == "draft"
	if release.get("draft") is not expected_draft:
		fail(f"release must be {release_state} during validation")
	if release.get("prerelease") is not False:
		fail("stable Rsnap releases must not be prereleases")
	release_url_prefix = f"https://github.com/{repository}/releases/tag/"
	release_url = release.get("html_url")
	if not isinstance(release_url, str) or not release_url.startswith(
		release_url_prefix
	):
		fail(f"release URL must use the canonical {repository} repository")
	release_slug = release_url.removeprefix(release_url_prefix)
	if release_state == "published":
		if release_slug != tag:
			fail(f"published release URL must use the canonical tag {tag}")
	elif release_slug != tag and re.fullmatch(
		r"untagged-[A-Za-z0-9][A-Za-z0-9._-]*", release_slug
	) is None:
		fail("draft release URL must use the tag or a safe GitHub untagged slug")

	assets = read_json(assets_path)
	if not isinstance(assets, list):
		fail("release assets API response must be an array")
	if len(assets) != len(local_assets):
		fail(
			"release assets do not match the exact expected count: "
			f"{len(assets)}"
		)
	assets_by_name = {
		asset.get("name"): asset
		for asset in assets
		if isinstance(asset, dict) and isinstance(asset.get("name"), str)
	}
	if set(assets_by_name) != set(local_assets):
		fail(
			"release assets do not match the exact expected set: "
			f"{sorted(assets_by_name)}"
		)
	for name, local_path in local_assets.items():
		asset = assets_by_name[name]
		if asset.get("state") != "uploaded":
			fail(f"release asset is not uploaded: {name}")
		if asset.get("size") != local_path.stat().st_size:
			fail(f"release asset size does not match local bytes: {name}")
		expected_url = (
			f"https://github.com/{repository}/releases/download/{release_slug}/{name}"
		)
		if asset.get("browser_download_url") != expected_url:
			fail(f"release asset URL must be {expected_url}")
		digest = asset.get("digest")
		if digest is not None and digest != f"sha256:{sha256(local_path)}":
			fail(f"release asset digest does not match local bytes: {name}")


def main() -> int:
	args = parse_args()
	if args.repository != CANONICAL_REPOSITORY:
		fail(
			f"release repository must be {CANONICAL_REPOSITORY}, got {args.repository}"
		)
	if STABLE_VERSION_RE.fullmatch(args.version) is None:
		fail(f"release version is not stable SemVer: {args.version}")
	if STABLE_VERSION_RE.fullmatch(args.sparkle_version) is None:
		fail(f"Sparkle version is not stable SemVer: {args.sparkle_version}")
	if args.tag != f"v{args.version}":
		fail(f"release tag {args.tag} does not match version {args.version}")

	if args.app is not None:
		validate_app(
			args.app,
			version=args.version,
			sparkle_version=args.sparkle_version,
			repository=args.repository,
		)

	artifact_paths = (args.archive, args.appcast, args.checksum)
	if any(path is not None for path in artifact_paths):
		if any(path is None for path in artifact_paths):
			fail("--archive, --appcast, and --checksum must be supplied together")
		archive, appcast, checksum = artifact_paths
		assert archive is not None and appcast is not None and checksum is not None
		validate_archive_bundle(
			archive,
			version=args.version,
			sparkle_version=args.sparkle_version,
			repository=args.repository,
		)
		validate_appcast(
			appcast,
			archive=archive,
			version=args.version,
			tag=args.tag,
			repository=args.repository,
			verify_signature=args.verify_appcast_signature,
		)
		validate_checksum(checksum, archive=archive)
	else:
		archive = appcast = checksum = None

	metadata_paths = (args.release_json, args.assets_json)
	if any(path is not None for path in metadata_paths):
		if any(path is None for path in metadata_paths):
			fail("--release-json and --assets-json must be supplied together")
		if archive is None or appcast is None or checksum is None:
			fail("release metadata validation requires all local release artifacts")
		if args.release_state is None:
			fail("release metadata validation requires --release-state")
		assert args.release_json is not None and args.assets_json is not None
		validate_release_metadata(
			args.release_json,
			args.assets_json,
			tag=args.tag,
			repository=args.repository,
			release_state=args.release_state,
			local_assets={
				ARCHIVE_NAME: archive,
				APPCAST_NAME: appcast,
				CHECKSUM_NAME: checksum,
			},
		)
	elif args.release_state is not None:
		fail("--release-state requires --release-json and --assets-json")

	if args.app is None and archive is None:
		fail("at least one of --app or --archive is required")
	return 0


if __name__ == "__main__":
	try:
		raise SystemExit(main())
	except ValidationError as error:
		print(f"error: {error}", file=sys.stderr)
		raise SystemExit(1) from error
