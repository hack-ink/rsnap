#!/usr/bin/env python3
"""Publish one validated Rsnap release through a GitHub draft."""

from __future__ import annotations

import filecmp
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ARCHIVE_NAME = "rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME = "appcast.xml"
CHECKSUM_NAME = f"{ARCHIVE_NAME}.sha256"
CANONICAL_REPOSITORY = "acg-box/rsnap"
ASSET_NAMES = (ARCHIVE_NAME, APPCAST_NAME, CHECKSUM_NAME)
SEMVER_PATTERN = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


class PublishError(RuntimeError):
    """Report an invalid release state or failed GitHub operation."""


def require(condition: bool, message: str) -> None:
    """Raise a publish error when a required condition is false."""
    if not condition:
        raise PublishError(message)


def required_environment(name: str) -> str:
    """Return one required, non-empty environment value."""
    value = os.environ.get(name, "").strip()
    if not value:
        raise PublishError(f"missing required environment variable: {name}")
    return value


class Publisher:
    """Own the draft-first GitHub publication sequence."""

    def __init__(self) -> None:
        self.repository = required_environment("GITHUB_REPOSITORY")
        self.release_commit = required_environment("RSNAP_RELEASE_COMMIT")
        self.tag = required_environment("RSNAP_RELEASE_TAG")
        self.version = required_environment("RSNAP_RELEASE_VERSION")
        self.input_dir = Path(required_environment("RSNAP_RELEASE_INPUT_DIR")).resolve()
        self.gh = os.environ.get("GH_BIN", "gh")
        self.validator = Path(
            os.environ.get(
                "RSNAP_RELEASE_VALIDATOR",
                Path(__file__).with_name("validate-release-artifacts.py"),
            )
        )

        required_environment("GH_TOKEN")
        require(
            self.repository == CANONICAL_REPOSITORY,
            f"GITHUB_REPOSITORY must be {CANONICAL_REPOSITORY}",
        )
        require(
            required_environment("GITHUB_SHA") == self.release_commit,
            "GITHUB_SHA does not match RSNAP_RELEASE_COMMIT",
        )
        require(self.input_dir.is_dir(), f"input directory not found: {self.input_dir}")
        require(self.validator.is_file(), f"validator not found: {self.validator}")

        self.artifacts = {
            ARCHIVE_NAME: self.input_dir / ARCHIVE_NAME,
            APPCAST_NAME: self.input_dir / APPCAST_NAME,
            CHECKSUM_NAME: self.input_dir / CHECKSUM_NAME,
        }
        for artifact in self.artifacts.values():
            require(artifact.is_file(), f"release artifact not found: {artifact}")

    def run_gh(
        self,
        arguments: list[str],
        *,
        capture_output: bool = True,
    ) -> subprocess.CompletedProcess[str]:
        """Run GitHub CLI without a shell."""
        result = subprocess.run(
            [self.gh, *arguments],
            check=False,
            capture_output=capture_output,
            text=True,
        )
        if result.returncode != 0:
            detail = (
                (result.stderr or "").strip()
                or (result.stdout or "").strip()
                or "unknown GitHub CLI error"
            )
            raise PublishError(f"gh {' '.join(arguments)} failed: {detail}")
        return result

    def gh_json(
        self,
        arguments: list[str],
        *,
        allow_not_found: bool = False,
    ) -> Any | None:
        """Run GitHub CLI and decode one JSON response."""
        result = subprocess.run(
            [self.gh, *arguments],
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            detail = result.stderr.strip() or result.stdout.strip()
            if allow_not_found and ("HTTP 404" in detail or "Not Found" in detail):
                return None
            raise PublishError(f"gh {' '.join(arguments)} failed: {detail}")
        try:
            return json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise PublishError("GitHub CLI returned invalid JSON") from error

    def validate_artifacts(self, artifacts: dict[str, Path]) -> None:
        """Validate one complete release artifact set."""
        result = subprocess.run(
            [
                str(self.validator),
                "--archive",
                str(artifacts[ARCHIVE_NAME]),
                "--appcast",
                str(artifacts[APPCAST_NAME]),
                "--checksum",
                str(artifacts[CHECKSUM_NAME]),
                "--version",
                self.version,
                "--tag",
                self.tag,
                "--repository",
                self.repository,
            ],
            check=False,
        )
        require(result.returncode == 0, "release artifact validation failed")

    def validate_local_artifacts(self) -> None:
        """Validate all local bytes before the first GitHub mutation."""
        self.validate_artifacts(self.artifacts)

    def validate_remote_tag(self) -> None:
        """Confirm that the remote tag still resolves to the validated commit."""
        metadata = self.gh_json(
            ["api", f"repos/{self.repository}/commits/{self.tag}"]
        )
        require(
            isinstance(metadata, dict) and metadata.get("sha") == self.release_commit,
            "remote release tag does not resolve to RSNAP_RELEASE_COMMIT",
        )

    def get_release(self) -> dict[str, Any] | None:
        """Return the same-tag release, if one exists."""
        metadata = self.gh_json(
            ["api", f"repos/{self.repository}/releases/tags/{self.tag}"],
            allow_not_found=True,
        )
        require(
            metadata is None or isinstance(metadata, dict),
            "GitHub release metadata must be an object",
        )
        return metadata

    def validate_release(
        self,
        release: dict[str, Any],
        *,
        draft: bool,
    ) -> int:
        """Validate the minimal same-tag release state and return its ID."""
        release_id = release.get("id")
        require(
            isinstance(release_id, int) and release_id > 0,
            "GitHub release ID is invalid",
        )
        require(release.get("tag_name") == self.tag, "GitHub release tag does not match")
        require(release.get("draft") is draft, "GitHub release draft state does not match")
        require(release.get("prerelease") is False, "GitHub release must not be a prerelease")
        return release_id

    def validate_next_version(self) -> None:
        """Require the new version to advance every published stable release."""
        published_versions: list[tuple[int, int, int]] = []
        page = 1
        while True:
            releases = self.gh_json(
                [
                    "api",
                    f"repos/{self.repository}/releases?per_page=100&page={page}",
                ]
            )
            require(isinstance(releases, list), "GitHub releases must be an array")
            for release in releases:
                require(isinstance(release, dict), "GitHub release metadata is invalid")
                if release.get("draft") is True or release.get("prerelease") is True:
                    continue
                match = SEMVER_PATTERN.fullmatch(str(release.get("tag_name", "")))
                if match is not None:
                    published_versions.append(tuple(int(part) for part in match.groups()))
            if len(releases) < 100:
                break
            page += 1

        if not published_versions:
            return
        target = tuple(int(part) for part in self.version.split("."))
        current = max(published_versions)
        require(
            target > current,
            f"release version {self.version} must be higher than "
            f"v{'.'.join(str(part) for part in current)}",
        )

    def create_draft(self) -> dict[str, Any]:
        """Create the same-tag draft release."""
        metadata = self.gh_json(
            [
                "api",
                "--method",
                "POST",
                f"repos/{self.repository}/releases",
                "-f",
                f"tag_name={self.tag}",
                "-f",
                f"target_commitish={self.release_commit}",
                "-F",
                "draft=true",
                "-F",
                "prerelease=false",
                "-F",
                "generate_release_notes=true",
            ]
        )
        require(isinstance(metadata, dict), "created release metadata is invalid")
        return metadata

    def fetch_assets(self, release_id: int) -> dict[str, int]:
        """Return the exact uploaded asset name-to-ID mapping."""
        metadata = self.gh_json(
            ["api", f"repos/{self.repository}/releases/{release_id}/assets?per_page=100"]
        )
        require(isinstance(metadata, list), "GitHub release assets must be an array")
        assets: dict[str, int] = {}
        for asset in metadata:
            require(isinstance(asset, dict), "GitHub release asset is invalid")
            name = asset.get("name")
            asset_id = asset.get("id")
            require(isinstance(name, str), "GitHub release asset name is invalid")
            require(
                isinstance(asset_id, int) and asset_id > 0,
                f"GitHub release asset ID is invalid: {name}",
            )
            require(name not in assets, f"duplicate GitHub release asset: {name}")
            assets[name] = asset_id
        return assets

    def delete_assets(self, assets: dict[str, int]) -> None:
        """Delete all assets from a reusable draft."""
        for asset_id in assets.values():
            self.run_gh(
                [
                    "api",
                    "--method",
                    "DELETE",
                    f"repos/{self.repository}/releases/assets/{asset_id}",
                    "--silent",
                ]
            )

    def upload_assets(self) -> None:
        """Upload the exact local artifact triplet."""
        self.run_gh(
            [
                "release",
                "upload",
                self.tag,
                "--repo",
                self.repository,
                *(str(self.artifacts[name]) for name in ASSET_NAMES),
            ],
            capture_output=False,
        )

    def download_assets(
        self,
        assets: dict[str, int],
        output_dir: Path,
    ) -> dict[str, Path]:
        """Download and return the exact release asset set."""
        require(set(assets) == set(ASSET_NAMES), "release must contain exactly three assets")
        output_dir.mkdir()
        downloads: dict[str, Path] = {}
        for name in ASSET_NAMES:
            destination = output_dir / name
            with destination.open("wb") as stream:
                result = subprocess.run(
                    [
                        self.gh,
                        "api",
                        "-H",
                        "Accept: application/octet-stream",
                        f"repos/{self.repository}/releases/assets/{assets[name]}",
                    ],
                    check=False,
                    stdout=stream,
                    stderr=subprocess.PIPE,
                )
            if result.returncode != 0:
                detail = result.stderr.decode(errors="replace").strip()
                raise PublishError(f"cannot download GitHub release asset {name}: {detail}")
            downloads[name] = destination
        return downloads

    def validate_remote_asset_bytes(self, release_id: int) -> None:
        """Require draft assets to match the validated local bytes."""
        with tempfile.TemporaryDirectory(prefix="rsnap-release-download-") as temp_dir:
            downloads = self.download_assets(
                self.fetch_assets(release_id),
                Path(temp_dir) / "assets",
            )
            for name in ASSET_NAMES:
                require(
                    filecmp.cmp(self.artifacts[name], downloads[name], shallow=False),
                    f"downloaded GitHub asset does not match local bytes: {name}",
                )

    def validate_published_assets(self, release_id: int) -> None:
        """Validate an existing public release from its downloaded assets."""
        with tempfile.TemporaryDirectory(prefix="rsnap-release-download-") as temp_dir:
            downloads = self.download_assets(
                self.fetch_assets(release_id),
                Path(temp_dir) / "assets",
            )
            self.validate_artifacts(downloads)

    def publish(self) -> None:
        """Execute the complete draft-first publication."""
        self.validate_local_artifacts()
        self.validate_remote_tag()

        release = self.get_release()
        if release is not None and release.get("draft") is False:
            release_id = self.validate_release(release, draft=False)
            self.validate_published_assets(release_id)
            print(f"GitHub release {self.tag} is already public and valid.")
            return

        self.validate_next_version()
        created = release is None
        if release is None:
            release = self.create_draft()
        release_id = self.validate_release(release, draft=True)

        self.delete_assets(self.fetch_assets(release_id))
        self.upload_assets()

        refreshed = self.get_release()
        require(refreshed is not None, "draft release disappeared after upload")
        require(
            self.validate_release(refreshed, draft=True) == release_id,
            "draft release ID changed after upload",
        )
        self.validate_remote_asset_bytes(release_id)

        print(
            f"{'Created' if created else 'Reused'} and validated draft release {self.tag}."
        )

        # Publication is intentionally the final GitHub operation.
        self.run_gh(
            [
                "api",
                "--method",
                "PATCH",
                f"repos/{self.repository}/releases/{release_id}",
                "-F",
                "draft=false",
                "-f",
                "make_latest=true",
                "--silent",
            ]
        )
        print(f"Published {self.tag} as the latest stable GitHub release.")


def main() -> int:
    """Publish the validated release or report one concise failure."""
    try:
        Publisher().publish()
    except (OSError, PublishError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
