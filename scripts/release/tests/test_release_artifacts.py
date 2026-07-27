from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
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
DRAFT_TOKEN = "untagged-01234567-89ab-cdef-0123-456789abcdef"


class ReleaseFixture:
    def __init__(
        self,
        root: pathlib.Path,
        *,
        production_public_key: bool = False,
    ) -> None:
        self.root = root
        self.version = "1.2.3"
        self.tag = "v1.2.3"
        self.archive = root / VALIDATOR.ARCHIVE_NAME
        self.appcast = root / VALIDATOR.APPCAST_NAME
        self.checksum = root / VALIDATOR.CHECKSUM_NAME
        self.private_key = root / "sparkle-private.pem"

        if production_public_key:
            self.public_key_b64 = VALIDATOR.SPARKLE_PUBLIC_ED_KEY
            self.private_key = None
        else:
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
            archive.writestr(
                "Rsnap.app/Contents/MacOS/RsnapNativeHost",
                b"test executable",
            )
            if extra_entry is not None:
                archive.writestr(extra_entry, b"unexpected")

    def signature(self) -> bytes:
        if self.private_key is None:
            return bytes(64)
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
            "verify_signature": True,
            "public_key_b64": self.public_key_b64,
        }
        arguments.update(overrides)
        VALIDATOR.validate_artifacts(**arguments)

    def release_metadata(self, *, draft: bool = True) -> dict[str, object]:
        download_tag = DRAFT_TOKEN if draft else self.tag
        return {
            "id": 42,
            "tag_name": self.tag,
            "target_commitish": "a" * 40,
            "draft": draft,
            "prerelease": False,
            "url": (
                f"https://api.github.com/repos/{VALIDATOR.CANONICAL_REPOSITORY}/"
                "releases/42"
            ),
            "html_url": (
                f"https://github.com/{VALIDATOR.CANONICAL_REPOSITORY}/"
                f"releases/tag/{download_tag}"
            ),
        }

    def asset_metadata(self, *, draft: bool = True) -> list[dict[str, object]]:
        download_tag = DRAFT_TOKEN if draft else self.tag
        result = []
        for asset_id, path in enumerate(
            (self.archive, self.appcast, self.checksum),
            start=101,
        ):
            result.append(
                {
                    "id": asset_id,
                    "name": path.name,
                    "size": path.stat().st_size,
                    "state": "uploaded",
                    "url": (
                        f"https://api.github.com/repos/"
                        f"{VALIDATOR.CANONICAL_REPOSITORY}/releases/assets/{asset_id}"
                    ),
                    "browser_download_url": (
                        f"https://github.com/{VALIDATOR.CANONICAL_REPOSITORY}/"
                        f"releases/download/{download_tag}/{path.name}"
                    ),
                }
            )
        return result


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

    def test_rejects_noncanonical_feed_url(self) -> None:
        fixture = ReleaseFixture(self.root)
        fixture.write_archive(feed_url="https://example.invalid/appcast.xml")
        fixture.write_appcast()
        fixture.write_checksum()
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "SUFeedURL"):
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

    def test_validates_optional_github_metadata(self) -> None:
        fixture = ReleaseFixture(self.root)
        release_json = self.root / "release.json"
        assets_json = self.root / "assets.json"
        release_json.write_text(json.dumps(fixture.release_metadata()), encoding="utf-8")
        assets_json.write_text(json.dumps(fixture.asset_metadata()), encoding="utf-8")
        fixture.validate(
            release_json=release_json,
            assets_json=assets_json,
            release_state="draft",
        )

        metadata = fixture.asset_metadata()
        metadata.append(metadata[0])
        assets_json.write_text(json.dumps(metadata), encoding="utf-8")
        with self.assertRaisesRegex(
            VALIDATOR.ValidationError,
            "exactly three assets",
        ):
            fixture.validate(
                release_json=release_json,
                assets_json=assets_json,
                release_state="draft",
            )

    def test_validates_published_github_urls(self) -> None:
        fixture = ReleaseFixture(self.root)
        release_json = self.root / "release.json"
        assets_json = self.root / "assets.json"
        release_json.write_text(
            json.dumps(fixture.release_metadata(draft=False)),
            encoding="utf-8",
        )
        assets_json.write_text(
            json.dumps(fixture.asset_metadata(draft=False)),
            encoding="utf-8",
        )
        fixture.validate(
            release_json=release_json,
            assets_json=assets_json,
            release_state="published",
        )

    def test_rejects_draft_asset_from_another_untagged_release(self) -> None:
        fixture = ReleaseFixture(self.root)
        release_json = self.root / "release.json"
        assets_json = self.root / "assets.json"
        release_json.write_text(json.dumps(fixture.release_metadata()), encoding="utf-8")
        assets = fixture.asset_metadata()
        assets[0]["browser_download_url"] = (
            f"https://github.com/{VALIDATOR.CANONICAL_REPOSITORY}/releases/"
            f"download/untagged-fedcba98-7654-3210-fedc-ba9876543210/"
            f"{VALIDATOR.ARCHIVE_NAME}"
        )
        assets_json.write_text(json.dumps(assets), encoding="utf-8")
        with self.assertRaisesRegex(
            VALIDATOR.ValidationError,
            "download URL is not canonical",
        ):
            fixture.validate(
                release_json=release_json,
                assets_json=assets_json,
                release_state="draft",
            )

    def test_cli_supports_required_contract(self) -> None:
        fixture = ReleaseFixture(self.root, production_public_key=True)
        result = subprocess.run(
            [
                sys.executable,
                str(VALIDATOR_PATH),
                "--archive",
                str(fixture.archive),
                "--appcast",
                str(fixture.appcast),
                "--checksum",
                str(fixture.checksum),
                "--version",
                fixture.version,
                "--tag",
                fixture.tag,
                "--repository",
                VALIDATOR.CANONICAL_REPOSITORY,
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
