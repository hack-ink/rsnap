#!/usr/bin/env python3
"""Validate the complete Rsnap GitHub release artifact set."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import pathlib
import plistlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import zipfile
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
SPARKLE_PUBLIC_ED_KEY = (
    pathlib.Path(__file__).with_name("sparkle-public-ed-key.txt").read_text().strip()
)
SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
SEMVER_PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
MAX_METADATA_BYTES = 4 * 1024 * 1024
MAX_ARCHIVE_ENTRIES = 50_000
MAX_ARCHIVE_UNCOMPRESSED_BYTES = 2 * 1024 * 1024 * 1024
MAX_ARCHIVE_ENTRY_BYTES = 512 * 1024 * 1024
MAX_ARCHIVE_PATH_BYTES = 4_096
MAX_ZIP_ENTRY_METADATA_BYTES = 64 * 1024
MAX_SYMLINK_TARGET_BYTES = 255
SPARKLE_COMPONENTS = (
    "XPCServices/Installer.xpc",
    "XPCServices/Downloader.xpc",
    "Autoupdate",
    "Updater.app",
)
SPARKLE_CODE_BUNDLES = {
    "XPCServices/Installer.xpc",
    "XPCServices/Downloader.xpc",
    "Updater.app",
}


class ValidationError(ValueError):
    """Report an invalid or inconsistent release artifact."""


def require(condition: bool, message: str) -> None:
    """Raise a release validation error when a condition is false."""
    if not condition:
        raise ValidationError(message)


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


def validate_app_info(
    info: object,
    version: str,
    public_key_b64: str,
) -> None:
    """Validate the public identity in one app Info.plist."""
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


def validate_sparkle_info(info: object, sparkle_version: str) -> None:
    """Validate the embedded Sparkle framework identity and version."""
    require(isinstance(info, dict), "Sparkle Info.plist root must be a dictionary")
    require(
        info.get("CFBundleIdentifier") == "org.sparkle-project.Sparkle",
        "embedded framework is not org.sparkle-project.Sparkle",
    )
    actual_version = info.get("CFBundleShortVersionString")
    require(
        actual_version == sparkle_version,
        f"embedded Sparkle version {actual_version!r} does not match {sparkle_version}",
    )


def resolve_sparkle_layout(app: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    """Resolve a dynamic, direct-child Sparkle Versions/Current layout."""
    framework = app / "Contents/Frameworks/Sparkle.framework"
    versions = framework / "Versions"
    current = versions / "Current"
    require(framework.is_dir() and versions.is_dir(), "Sparkle.framework layout is missing")
    require(current.is_symlink(), "Sparkle Versions/Current must be a symbolic link")
    try:
        versions_real = versions.resolve(strict=True)
        current_real = current.resolve(strict=True)
    except OSError as error:
        raise ValidationError(f"cannot resolve Sparkle Versions/Current: {error}") from error
    require(
        current_real.parent == versions_real and current_real.is_dir(),
        "Sparkle Versions/Current must resolve to a direct Versions child",
    )
    require(
        re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", current_real.name) is not None,
        "Sparkle version directory name is unsafe",
    )
    version_entries = list(versions.iterdir())
    require(
        {entry.name for entry in version_entries} == {"Current", current_real.name},
        "Sparkle Versions must contain only Current and its version directory",
    )
    version_directories = [
        entry.resolve(strict=True)
        for entry in version_entries
        if entry.name != "Current" and entry.is_dir() and not entry.is_symlink()
    ]
    require(
        version_directories == [current_real],
        "Sparkle Versions must contain exactly the Current version directory",
    )
    require(
        current.readlink().as_posix() == current_real.name,
        "Sparkle Versions/Current must use a safe single-segment target",
    )
    return framework, current_real


def validate_sparkle_graph(app: pathlib.Path, sparkle_version: str) -> None:
    """Validate the known Sparkle 2.9.5 code graph without hard-coding its version letter."""
    framework, version_root = resolve_sparkle_layout(app)
    for relative_path in SPARKLE_COMPONENTS:
        require(
            (version_root / relative_path).exists(),
            f"embedded Sparkle code is missing: {relative_path}",
        )
    autoupdate = version_root / "Autoupdate"
    require(
        autoupdate.is_file() and bool(autoupdate.stat().st_mode & stat.S_IXUSR),
        "Sparkle Autoupdate must be an executable file",
    )
    discovered_bundles = {
        path.relative_to(version_root).as_posix()
        for path in version_root.rglob("*")
        if path.is_dir() and path.suffix in {".app", ".xpc", ".framework"}
    }
    require(
        discovered_bundles == SPARKLE_CODE_BUNDLES,
        "Sparkle framework contains an unknown nested code bundle",
    )
    try:
        with (version_root / "Resources/Info.plist").open("rb") as stream:
            sparkle_info = plistlib.load(stream)
    except (OSError, plistlib.InvalidFileException) as error:
        raise ValidationError(f"cannot read embedded Sparkle Info.plist: {error}") from error
    validate_sparkle_info(sparkle_info, sparkle_version)
    require(framework.is_dir(), "Sparkle.framework is missing")


def validate_app(
    app: pathlib.Path,
    version: str,
    sparkle_version: str,
    public_key_b64: str,
) -> None:
    """Validate one staged app before release signing."""
    require(app.is_dir() and app.name == APP_BUNDLE_NAME, f"app must be {APP_BUNDLE_NAME}")
    try:
        with (app / "Contents/Info.plist").open("rb") as stream:
            info = plistlib.load(stream)
    except (OSError, plistlib.InvalidFileException) as error:
        raise ValidationError(f"cannot read app Info.plist: {error}") from error
    validate_app_info(info, version, public_key_b64)
    validate_sparkle_graph(app, sparkle_version)


def validate_archive_sparkle_graph(
    bundle_zip: zipfile.ZipFile,
    names: set[str],
    sparkle_version: str,
) -> None:
    """Validate the dynamic Sparkle code graph in an unextracted release ZIP."""
    versions_prefix = (
        f"{APP_BUNDLE_NAME}/Contents/Frameworks/Sparkle.framework/Versions/"
    )
    info_matches = [
        name
        for name in names
        if re.fullmatch(
            re.escape(versions_prefix) + r"([^/]+)/Resources/Info\.plist",
            name,
        )
    ]
    require(
        len(info_matches) == 1,
        "release archive must contain exactly one Sparkle version directory",
    )
    info_name = info_matches[0]
    version_name = pathlib.PurePosixPath(info_name).parts[-3]
    require(
        re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", version_name) is not None
        and version_name != "Current",
        "release archive has an invalid Sparkle version directory",
    )
    version_prefix = f"{versions_prefix}{version_name}/"
    direct_children = {
        remainder.split("/", maxsplit=1)[0]
        for name in names
        if name.startswith(versions_prefix)
        for remainder in [name.removeprefix(versions_prefix).rstrip("/")]
        if remainder
    }
    require(
        direct_children == {"Current", version_name},
        "release archive Sparkle Versions must contain only Current and its version directory",
    )
    for relative_path in SPARKLE_COMPONENTS:
        require(
            any(
                name == f"{version_prefix}{relative_path}"
                or name.startswith(f"{version_prefix}{relative_path}/")
                for name in names
            ),
            f"release archive is missing Sparkle code: {relative_path}",
        )

    discovered_bundles: set[str] = set()
    for name in names:
        if not name.startswith(version_prefix):
            continue
        relative_parts = pathlib.PurePosixPath(name.removeprefix(version_prefix)).parts
        for index, part in enumerate(relative_parts):
            if part.endswith((".app", ".xpc", ".framework")):
                discovered_bundles.add("/".join(relative_parts[: index + 1]))
                break
    require(
        discovered_bundles == SPARKLE_CODE_BUNDLES,
        "release archive contains an unknown Sparkle code bundle",
    )

    current_name = f"{versions_prefix}Current"
    require(current_name in names, "release archive is missing Sparkle Versions/Current")
    current_entry = bundle_zip.getinfo(current_name)
    require(
        stat.S_ISLNK(current_entry.external_attr >> 16),
        "Sparkle Versions/Current in the release archive must be a symlink",
    )
    require(
        current_entry.file_size <= MAX_SYMLINK_TARGET_BYTES,
        "Sparkle Versions/Current target is too large",
    )
    try:
        current_target = bundle_zip.read(current_entry).decode("utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise ValidationError(f"cannot read Sparkle Versions/Current: {error}") from error
    require(
        current_target == version_name,
        "Sparkle Versions/Current must name the single version directory",
    )
    info_entry = bundle_zip.getinfo(info_name)
    require(info_entry.file_size <= MAX_METADATA_BYTES, "Sparkle Info.plist is too large")
    try:
        sparkle_info = plistlib.loads(bundle_zip.read(info_entry))
    except (OSError, plistlib.InvalidFileException) as error:
        raise ValidationError(f"cannot parse Sparkle Info.plist: {error}") from error
    validate_sparkle_info(sparkle_info, sparkle_version)


def validate_archive(
    archive: pathlib.Path,
    version: str,
    public_key_b64: str,
    sparkle_version: str | None = None,
) -> None:
    """Validate the app root and its release identity without extracting the ZIP."""
    try:
        with zipfile.ZipFile(archive) as bundle_zip:
            entries = bundle_zip.infolist()
            require(entries, "release archive is empty")
            require(
                len(entries) <= MAX_ARCHIVE_ENTRIES,
                "release archive contains too many entries",
            )
            require(
                len(bundle_zip.comment) <= MAX_ZIP_ENTRY_METADATA_BYTES,
                "release archive comment is too large",
            )
            total_uncompressed_bytes = sum(entry.file_size for entry in entries)
            require(
                total_uncompressed_bytes <= MAX_ARCHIVE_UNCOMPRESSED_BYTES,
                "release archive declared uncompressed size is too large",
            )

            names: set[str] = set()
            payload_names: list[str] = []
            for entry in entries:
                name = entry.filename
                require(
                    entry.file_size <= MAX_ARCHIVE_ENTRY_BYTES,
                    f"release archive entry is too large: {name}",
                )
                require(
                    len(name.encode("utf-8")) <= MAX_ARCHIVE_PATH_BYTES,
                    "release archive entry path is too large",
                )
                require(
                    len(entry.extra) <= MAX_ZIP_ENTRY_METADATA_BYTES
                    and len(entry.comment) <= MAX_ZIP_ENTRY_METADATA_BYTES,
                    f"release archive entry metadata is too large: {name}",
                )
                require(name not in names, f"release archive has duplicate entry: {name}")
                names.add(name)
                require("\x00" not in name, "release archive has a NUL in an entry name")
                require(
                    not any(ord(character) < 32 or ord(character) == 127 for character in name),
                    f"release archive has a control character in an entry name: {name!r}",
                )
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
            if sparkle_version is not None:
                validate_archive_sparkle_graph(
                    bundle_zip,
                    names,
                    sparkle_version,
                )
    except (OSError, zipfile.BadZipFile, KeyError) as error:
        raise ValidationError(f"cannot read release archive: {error}") from error

    validate_app_info(info, version, public_key_b64)


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


def validate_artifacts(
    *,
    archive: pathlib.Path,
    appcast: pathlib.Path,
    checksum: pathlib.Path,
    version: str,
    tag: str,
    repository: str,
    public_key_b64: str = SPARKLE_PUBLIC_ED_KEY,
    sparkle_version: str | None = None,
) -> None:
    """Validate the complete local release artifact set."""
    validate_identity(archive, appcast, checksum, version, tag, repository)
    validate_archive(archive, version, public_key_b64, sparkle_version)
    validate_appcast(
        appcast,
        archive,
        version,
        tag,
        public_key_b64,
    )
    validate_checksum(checksum, archive)


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse the release validator command line."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=pathlib.Path)
    parser.add_argument("--archive", type=pathlib.Path)
    parser.add_argument("--appcast", type=pathlib.Path)
    parser.add_argument("--checksum", type=pathlib.Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--sparkle-version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the release artifact validator."""
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        require(
            SEMVER_PATTERN.fullmatch(args.sparkle_version) is not None,
            "Sparkle version must be X.Y.Z",
        )
        if args.app is not None:
            require(
                args.archive is None and args.appcast is None and args.checksum is None,
                "--app cannot be combined with archive artifacts",
            )
            require(args.repository == CANONICAL_REPOSITORY, "repository must be acg-box/rsnap")
            require(args.tag == f"v{args.version}", "tag must be v followed by the release version")
            validate_app(
                args.app,
                args.version,
                args.sparkle_version,
                SPARKLE_PUBLIC_ED_KEY,
            )
        else:
            require(
                args.archive is not None
                and args.appcast is not None
                and args.checksum is not None,
                "--archive, --appcast, and --checksum are required together",
            )
            validate_artifacts(
                archive=args.archive,
                appcast=args.appcast,
                checksum=args.checksum,
                version=args.version,
                sparkle_version=args.sparkle_version,
                tag=args.tag,
                repository=args.repository,
            )
    except ValidationError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"validated Rsnap release artifacts for {args.tag}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
