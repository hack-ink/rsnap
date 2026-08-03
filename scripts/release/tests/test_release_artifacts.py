from __future__ import annotations

import base64
import hashlib
import importlib.util
import pathlib
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[3]
VALIDATOR_PATH = REPOSITORY_ROOT / "scripts/release/validate-release-artifacts.py"
SPEC = importlib.util.spec_from_file_location("rsnap_release_validator", VALIDATOR_PATH)
assert SPEC is not None and SPEC.loader is not None
VALIDATOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = VALIDATOR
SPEC.loader.exec_module(VALIDATOR)
class ReleaseFixture:
    def __init__(
        self,
        root: pathlib.Path,
    ) -> None:
        self.root = root
        self.version = "1.2.3"
        self.tag = "v1.2.3"
        self.archive = root / VALIDATOR.ARCHIVE_NAME
        self.appcast = root / VALIDATOR.APPCAST_NAME
        self.checksum = root / VALIDATOR.CHECKSUM_NAME
        self.private_key = root / "sparkle-private.pem"

        subprocess.run(
            [
                "openssl",
                "genpkey",
                "-algorithm",
                "ED25519",
                "-out",
                str(self.private_key),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        public_der = subprocess.run(
            [
                "openssl",
                "pkey",
                "-in",
                str(self.private_key),
                "-pubout",
                "-outform",
                "DER",
            ],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        ).stdout
        self.public_key_b64 = base64.b64encode(public_der[-32:]).decode()
        self.write_archive()
        self.write_appcast()
        self.write_checksum()

    def write_archive(
        self,
        *,
        feed_url: str = VALIDATOR.SPARKLE_FEED_URL,
        extra_entry: str | None = None,
        extra_sparkle_entry: str | None = None,
    ) -> None:
        info = {
            "CFBundleName": "Rsnap",
            "CFBundleDisplayName": "Rsnap",
            "CFBundleIdentifier": VALIDATOR.BUNDLE_IDENTIFIER,
            "CFBundleShortVersionString": self.version,
            "CFBundleVersion": self.version,
            "SUFeedURL": feed_url,
            "SUPublicEDKey": self.public_key_b64,
        }
        with zipfile.ZipFile(self.archive, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr(
                "Rsnap.app/Contents/Info.plist",
                plistlib.dumps(info),
            )
            archive.writestr(
                "Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle",
                b"test framework",
            )
            sparkle_root = (
                "Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/B"
            )
            archive.writestr(
                f"{sparkle_root}/Resources/Info.plist",
                plistlib.dumps(
                    {
                        "CFBundleIdentifier": "org.sparkle-project.Sparkle",
                        "CFBundleShortVersionString": "2.9.5",
                    }
                ),
            )
            for component in (
                "XPCServices/Installer.xpc/Contents/MacOS/Installer",
                "XPCServices/Downloader.xpc/Contents/MacOS/Downloader",
                "Autoupdate",
                "Updater.app/Contents/MacOS/Updater",
            ):
                archive.writestr(f"{sparkle_root}/{component}", b"test code")
            current = zipfile.ZipInfo(
                "Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/Current"
            )
            current.create_system = 3
            current.external_attr = (0o120777 << 16)
            archive.writestr(current, b"B")
            archive.writestr(
                "Rsnap.app/Contents/MacOS/RsnapNativeHost",
                b"test executable",
            )
            if extra_sparkle_entry is not None:
                archive.writestr(
                    "Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/"
                    f"{extra_sparkle_entry}",
                    b"unexpected",
                )
            if extra_entry is not None:
                archive.writestr(extra_entry, b"unexpected")

    def signature(self) -> bytes:
        signature_path = self.root / "signature.bin"
        subprocess.run(
            [
                "openssl",
                "pkeyutl",
                "-sign",
                "-inkey",
                str(self.private_key),
                "-rawin",
                "-in",
                str(self.archive),
                "-out",
                str(signature_path),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return signature_path.read_bytes()

    def write_appcast(
        self,
        *,
        signature: bytes | None = None,
        channel: str = "",
    ) -> None:
        signature_text = base64.b64encode(
            self.signature() if signature is None else signature
        ).decode()
        release_url = (
            f"https://github.com/{VALIDATOR.CANONICAL_REPOSITORY}/"
            f"releases/tag/{self.tag}"
        )
        archive_url = (
            f"https://github.com/{VALIDATOR.CANONICAL_REPOSITORY}/"
            f"releases/download/{self.tag}/{VALIDATOR.ARCHIVE_NAME}"
        )
        channel_element = (
            f"<sparkle:channel>{channel}</sparkle:channel>" if channel else ""
        )
        self.appcast.write_text(
            f"""<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"
  xmlns:sparkle="{VALIDATOR.SPARKLE_NAMESPACE}">
  <channel>
    <title>Rsnap Updates</title>
    <item>
      <title>Version {self.version}</title>
      <link>{release_url}</link>
      <sparkle:version>{self.version}</sparkle:version>
      <sparkle:shortVersionString>{self.version}</sparkle:shortVersionString>
      <sparkle:releaseNotesLink>{release_url}</sparkle:releaseNotesLink>
      {channel_element}
      <enclosure
        url="{archive_url}"
        sparkle:edSignature="{signature_text}"
        length="{self.archive.stat().st_size}"
        type="application/octet-stream" />
    </item>
  </channel>
</rss>
""",
            encoding="utf-8",
        )

    def write_checksum(self, *, separator: str = "  ") -> None:
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.checksum.write_text(
            f"{digest}{separator}{VALIDATOR.ARCHIVE_NAME}\n",
            encoding="ascii",
        )

    def validate(self, **overrides: object) -> None:
        arguments: dict[str, object] = {
            "archive": self.archive,
            "appcast": self.appcast,
            "checksum": self.checksum,
            "version": self.version,
            "tag": self.tag,
            "repository": VALIDATOR.CANONICAL_REPOSITORY,
            "public_key_b64": self.public_key_b64,
            "sparkle_version": "2.9.5",
        }
        arguments.update(overrides)
        VALIDATOR.validate_artifacts(**arguments)

@unittest.skipUnless(shutil.which("openssl"), "openssl is required")
class ReleaseArtifactTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary_directory.name)

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def test_valid_artifacts_and_signature(self) -> None:
        fixture = ReleaseFixture(self.root)
        fixture.validate()

    def test_rejects_second_zip_payload_root(self) -> None:
        fixture = ReleaseFixture(self.root)
        fixture.write_archive(extra_entry="README.txt")
        fixture.write_appcast()
        fixture.write_checksum()
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "second payload root"):
            fixture.validate()

    def test_rejects_archive_entry_count_over_bound(self) -> None:
        fixture = ReleaseFixture(self.root)
        original_bound = VALIDATOR.MAX_ARCHIVE_ENTRIES
        try:
            VALIDATOR.MAX_ARCHIVE_ENTRIES = 2
            with self.assertRaisesRegex(VALIDATOR.ValidationError, "too many entries"):
                fixture.validate()
        finally:
            VALIDATOR.MAX_ARCHIVE_ENTRIES = original_bound

    def test_rejects_noncanonical_feed_url(self) -> None:
        fixture = ReleaseFixture(self.root)
        fixture.write_archive(feed_url="https://example.invalid/appcast.xml")
        fixture.write_appcast()
        fixture.write_checksum()
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "SUFeedURL"):
            fixture.validate()

    def test_rejects_extra_sparkle_versions_entry(self) -> None:
        fixture = ReleaseFixture(self.root)
        fixture.write_archive(extra_sparkle_entry="A/ignored")
        fixture.write_appcast()
        fixture.write_checksum()
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "contain only Current"):
            fixture.validate()

    def test_rejects_invalid_signature(self) -> None:
        fixture = ReleaseFixture(self.root)
        signature = bytearray(fixture.signature())
        signature[0] ^= 0x01
        fixture.write_appcast(signature=bytes(signature))
        with self.assertRaisesRegex(
            VALIDATOR.ValidationError,
            "signature verification failed",
        ):
            fixture.validate()

    def test_rejects_noncanonical_checksum_format(self) -> None:
        fixture = ReleaseFixture(self.root)
        fixture.write_checksum(separator=" ")
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "exact SHA-256"):
            fixture.validate()

    def test_rejects_nonstable_appcast_channel(self) -> None:
        fixture = ReleaseFixture(self.root)
        fixture.write_appcast(channel="beta")
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "stable channel"):
            fixture.validate()

if __name__ == "__main__":
    unittest.main()
