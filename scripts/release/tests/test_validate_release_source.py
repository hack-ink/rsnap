#!/usr/bin/env python3
"""Tests for the Rsnap release-source validator."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

VALIDATOR = Path(__file__).resolve().parents[1] / "validate-release-source.py"


class ReleaseSourceValidatorTests(unittest.TestCase):
    """Exercise release provenance and version validation in temporary repositories."""

    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary_directory.cleanup)
        self.repo = Path(self.temporary_directory.name)
        self._write_fixture()
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Rsnap Release Test")
        self.git("config", "user.email", "release-test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "tag.gpgsign", "false")
        self.git("add", ".")
        self.release_commit = self.commit("release source")
        self.git("update-ref", "refs/remotes/origin/main", self.release_commit)
        self.git("tag", "-a", "v1.2.3", "-m", "release v1.2.3")

    def _write_fixture(self) -> None:
        (self.repo / "apps/rsnap").mkdir(parents=True)
        (self.repo / "packages/rsnap-core").mkdir(parents=True)
        (self.repo / "native/macos-host").mkdir(parents=True)
        (self.repo / "Cargo.toml").write_text(
            """
[workspace]
members = ["apps/*", "packages/*"]
resolver = "3"

[workspace.package]
version = "1.2.3"
""".lstrip(),
            encoding="utf-8",
        )
        for relative_path, name in (
            ("apps/rsnap/Cargo.toml", "rsnap"),
            ("packages/rsnap-core/Cargo.toml", "rsnap-core"),
        ):
            (self.repo / relative_path).write_text(
                f"""
[package]
name = "{name}"
version.workspace = true
""".lstrip(),
                encoding="utf-8",
            )
        (self.repo / "Cargo.lock").write_text(
            """
version = 4

[[package]]
name = "rsnap"
version = "1.2.3"

[[package]]
name = "rsnap-core"
version = "1.2.3"
""".lstrip(),
            encoding="utf-8",
        )
        (self.repo / "native/macos-host/Package.swift").write_text(
            """
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "RsnapNativeHost",
    dependencies: [
        .package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.9.4"),
    ]
)
""".lstrip(),
            encoding="utf-8",
        )
        (self.repo / "native/macos-host/Package.resolved").write_text(
            json.dumps(
                {
                    "pins": [
                        {
                            "identity": "sparkle",
                            "kind": "remoteSourceControl",
                            "location": "https://github.com/sparkle-project/Sparkle",
                            "state": {
                                "revision": "b6496a74a087257ef5e6da1c5b29a447a60f5bd7",
                                "version": "2.9.4",
                            },
                        }
                    ],
                    "version": 3,
                }
            ),
            encoding="utf-8",
        )

    def git(self, *arguments: str) -> str:
        """Run Git in the temporary repository."""
        return subprocess.run(
            ["git", *arguments],
            cwd=self.repo,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

    def git_with_input(self, content: str, *arguments: str) -> str:
        """Run Git with text supplied on standard input."""
        return subprocess.run(
            ["git", *arguments],
            cwd=self.repo,
            input=content,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

    def commit(self, message: str, parent: str | None = None) -> str:
        """Create a test commit without invoking repository commit automation."""
        tree = self.git("write-tree")
        arguments = ["commit-tree", tree, "-m", message]
        if parent is not None:
            arguments.extend(["-p", parent])
        commit = self.git(*arguments)
        self.git("update-ref", "HEAD", commit)
        return commit

    def run_validator(
        self,
        *,
        tag: str = "v1.2.3",
        repository: str = "acg-box/rsnap",
    ) -> subprocess.CompletedProcess[str]:
        """Run the validator with a representative GitHub tag event."""
        tag_object = self.git("rev-parse", f"refs/tags/{tag}")
        checkout_commit = self.git("rev-parse", "HEAD^{commit}")
        event_path = self.repo / "event.json"
        event_path.write_text(
            json.dumps(
                {
                    "after": tag_object,
                    "ref": f"refs/tags/{tag}",
                    "repository": {"full_name": repository},
                }
            ),
            encoding="utf-8",
        )
        output_path = self.repo / "github-output.txt"
        output_path.unlink(missing_ok=True)
        environment = os.environ.copy()
        environment.update(
            {
                "GITHUB_EVENT_PATH": str(event_path),
                "GITHUB_OUTPUT": str(output_path),
                "GITHUB_REF": f"refs/tags/{tag}",
                "GITHUB_REF_NAME": tag,
                "GITHUB_REPOSITORY": repository,
                "GITHUB_SHA": checkout_commit,
            }
        )
        return subprocess.run(
            [sys.executable, str(VALIDATOR)],
            cwd=self.repo,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )

    def assert_validation_fails(
        self, result: subprocess.CompletedProcess[str], message: str
    ) -> None:
        """Assert a validation failure contains the expected diagnostic."""
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)

    def test_valid_annotated_tag_writes_trusted_outputs(self) -> None:
        result = self.run_validator()

        self.assertEqual(result.returncode, 0, result.stderr)
        outputs = (self.repo / "github-output.txt").read_text(encoding="utf-8")
        self.assertEqual(
            outputs.splitlines(),
            [
                "version=1.2.3",
                f"tag_commit={self.release_commit}",
                "sparkle_version=2.9.4",
            ],
        )

    def test_lightweight_tag_is_rejected(self) -> None:
        self.git("tag", "-d", "v1.2.3")
        self.git("tag", "v1.2.3")

        result = self.run_validator()

        self.assert_validation_fails(result, "must be an annotated tag")

    def test_annotated_tag_header_name_must_match_ref_name(self) -> None:
        tag_content = self.git("cat-file", "-p", "refs/tags/v1.2.3")
        mismatched_content = tag_content.replace(
            "\ntag v1.2.3\n", "\ntag v9.9.9\n", 1
        )
        mismatched_tag_object = self.git_with_input(mismatched_content, "mktag")
        self.git("update-ref", "refs/tags/v1.2.3", mismatched_tag_object)

        result = self.run_validator()

        self.assert_validation_fails(
            result,
            "annotated tag name 'v9.9.9' does not match GITHUB_REF_NAME v1.2.3",
        )

    def test_tag_and_cargo_version_mismatch_is_rejected(self) -> None:
        self.git("tag", "-a", "v1.2.4", "-m", "wrong release version")

        result = self.run_validator(tag="v1.2.4")

        self.assert_validation_fails(
            result, "tag version 1.2.4 does not match Cargo version 1.2.3"
        )

    def test_tag_commit_outside_origin_main_is_rejected(self) -> None:
        (self.repo / "side-branch.txt").write_text("not on main\n", encoding="utf-8")
        self.git("add", "side-branch.txt")
        self.commit("side branch release", self.git("rev-parse", "HEAD"))
        self.git("tag", "-d", "v1.2.3")
        self.git("tag", "-a", "v1.2.3", "-m", "side branch release")

        result = self.run_validator()

        self.assert_validation_fails(result, "is not reachable from origin/main")

    def test_origin_main_can_advance_after_the_tag(self) -> None:
        (self.repo / "later.txt").write_text("main moved forward\n", encoding="utf-8")
        self.git("add", "later.txt")
        later_commit = self.commit("later main commit", self.git("rev-parse", "HEAD"))
        self.git("update-ref", "refs/remotes/origin/main", later_commit)
        self.git("checkout", "--detach", self.release_commit)

        result = self.run_validator()

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_tag_with_leading_zero_is_rejected(self) -> None:
        self.git("tag", "-a", "v01.2.3", "-m", "invalid release version")

        result = self.run_validator(tag="v01.2.3")

        self.assert_validation_fails(
            result, "stable vX.Y.Z SemVer without leading zeroes"
        )

    def test_mismatched_sparkle_pin_is_rejected(self) -> None:
        resolved_path = self.repo / "native/macos-host/Package.resolved"
        resolved = json.loads(resolved_path.read_text(encoding="utf-8"))
        resolved["pins"][0]["state"]["version"] = "2.9.5"
        resolved_path.write_text(json.dumps(resolved), encoding="utf-8")

        result = self.run_validator()

        self.assert_validation_fails(
            result,
            "Package.resolved Sparkle version does not match Package.swift exact version",
        )

    def test_sparkle_exact_version_with_leading_zero_is_rejected(self) -> None:
        package_path = self.repo / "native/macos-host/Package.swift"
        package_source = package_path.read_text(encoding="utf-8")
        package_path.write_text(
            package_source.replace('exact: "2.9.4"', 'exact: "02.9.4"'),
            encoding="utf-8",
        )

        result = self.run_validator()

        self.assert_validation_fails(
            result,
            "Sparkle exact version must be stable X.Y.Z SemVer without leading zeroes",
        )


if __name__ == "__main__":
    unittest.main()
