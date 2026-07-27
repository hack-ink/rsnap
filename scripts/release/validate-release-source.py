#!/usr/bin/env python3
"""Validate that a GitHub tag build comes from an eligible Rsnap source commit."""

from __future__ import annotations

import glob
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

CANONICAL_REPOSITORY = "acg-box/rsnap"
STABLE_VERSION_PATTERN = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")
STABLE_TAG_PATTERN = re.compile(
    r"v((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))"
)
SPARKLE_LOCATION = "https://github.com/sparkle-project/Sparkle"
SPARKLE_EXACT_PATTERN = re.compile(
    r"""
    \.package
    \s*\(
    \s*url:\s*"https://github\.com/sparkle-project/Sparkle(?:\.git)?"
    \s*,\s*exact:\s*"([^"]+)"
    \s*\)
    """,
    re.VERBOSE | re.DOTALL,
)


class ValidationError(RuntimeError):
    """A release-source validation failure."""


def require_environment(name: str) -> str:
    """Return a required, non-empty environment variable."""
    value = os.environ.get(name, "").strip()
    if not value:
        raise ValidationError(f"{name} is required")
    return value


def git(repo_root: Path, *arguments: str) -> str:
    """Run a read-only Git command and return its trimmed stdout."""
    result = subprocess.run(
        ["git", *arguments],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "unknown Git error"
        raise ValidationError(f"git {' '.join(arguments)} failed: {detail}")
    return result.stdout.strip()


def load_toml(path: Path) -> dict[str, Any]:
    """Load a TOML document and require a table at its root."""
    with path.open("rb") as stream:
        document = tomllib.load(stream)
    if not isinstance(document, dict):
        raise ValidationError(f"{path} does not contain a TOML table")
    return document


def validate_github_event(tag: str) -> tuple[dict[str, Any], str]:
    """Validate canonical GitHub repository and tag event metadata."""
    repository = require_environment("GITHUB_REPOSITORY")
    if repository != CANONICAL_REPOSITORY:
        raise ValidationError(
            f"GITHUB_REPOSITORY must be {CANONICAL_REPOSITORY}, got {repository}"
        )

    github_ref = require_environment("GITHUB_REF")
    expected_ref = f"refs/tags/{tag}"
    if github_ref != expected_ref:
        raise ValidationError(f"GITHUB_REF must be {expected_ref}, got {github_ref}")

    event_path = Path(require_environment("GITHUB_EVENT_PATH"))
    try:
        event = json.loads(event_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValidationError(f"cannot read GitHub event payload: {error}") from error
    if not isinstance(event, dict):
        raise ValidationError("GitHub event payload must be a JSON object")

    if event.get("ref") != expected_ref:
        raise ValidationError(f"GitHub event ref must be {expected_ref}")
    event_repository = event.get("repository")
    if not isinstance(event_repository, dict):
        raise ValidationError("GitHub event repository metadata is missing")
    if event_repository.get("full_name") != CANONICAL_REPOSITORY:
        raise ValidationError(
            f"GitHub event repository must be {CANONICAL_REPOSITORY}"
        )

    event_after = event.get("after")
    if not isinstance(event_after, str) or not event_after:
        raise ValidationError("GitHub event after object is missing")
    return event, event_after


def validate_tag_and_source(repo_root: Path, tag: str, event_after: str) -> str:
    """Validate annotated-tag identity and main-branch ancestry."""
    tag_ref = f"refs/tags/{tag}"
    if git(repo_root, "cat-file", "-t", tag_ref) != "tag":
        raise ValidationError(f"{tag} must be an annotated tag")

    tag_object = git(repo_root, "cat-file", "-p", tag_ref)
    tag_headers = tag_object.partition("\n\n")[0].splitlines()
    header_values = {
        key: value
        for line in tag_headers
        if " " in line
        for key, value in [line.split(" ", 1)]
    }
    if header_values.get("type") != "commit":
        raise ValidationError(f"{tag} must point directly to a commit")
    annotated_tag_name = header_values.get("tag")
    if annotated_tag_name != tag:
        raise ValidationError(
            f"annotated tag name {annotated_tag_name!r} does not match GITHUB_REF_NAME {tag}"
        )

    direct_target = header_values.get("object", "")
    tag_commit = git(repo_root, "rev-parse", f"{tag_ref}^{{commit}}")
    if direct_target != tag_commit:
        raise ValidationError(f"{tag} does not point directly to its peeled commit")

    head_commit = git(repo_root, "rev-parse", "HEAD^{commit}")
    github_commit = git(
        repo_root, "rev-parse", f"{require_environment('GITHUB_SHA')}^{{commit}}"
    )
    event_commit = git(repo_root, "rev-parse", f"{event_after}^{{commit}}")
    source_commits = {
        "tag": tag_commit,
        "checkout": head_commit,
        "GITHUB_SHA": github_commit,
        "event": event_commit,
    }
    if len(set(source_commits.values())) != 1:
        details = ", ".join(f"{name}={commit}" for name, commit in source_commits.items())
        raise ValidationError(f"release source commits do not match: {details}")

    origin_main = git(repo_root, "rev-parse", "refs/remotes/origin/main^{commit}")
    ancestry = subprocess.run(
        ["git", "merge-base", "--is-ancestor", tag_commit, origin_main],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if ancestry.returncode == 1:
        raise ValidationError(f"tag commit {tag_commit} is not reachable from origin/main")
    if ancestry.returncode != 0:
        detail = ancestry.stderr.strip() or ancestry.stdout.strip() or "unknown Git error"
        raise ValidationError(f"cannot verify origin/main ancestry: {detail}")

    return tag_commit


def workspace_member_manifests(
    repo_root: Path, workspace: dict[str, Any]
) -> list[Path]:
    """Resolve Cargo workspace member patterns to package manifests."""
    members = workspace.get("members")
    if not isinstance(members, list) or not members:
        raise ValidationError("Cargo.toml workspace.members must be a non-empty array")

    manifests: set[Path] = set()
    for member_pattern in members:
        if not isinstance(member_pattern, str) or not member_pattern:
            raise ValidationError("Cargo.toml workspace member patterns must be strings")
        for match in glob.glob(str(repo_root / member_pattern)):
            candidate = Path(match)
            manifest = candidate if candidate.name == "Cargo.toml" else candidate / "Cargo.toml"
            if manifest.is_file():
                manifests.add(manifest.resolve())
    if not manifests:
        raise ValidationError("Cargo.toml workspace patterns did not resolve any packages")
    return sorted(manifests)


def validate_cargo_versions(repo_root: Path, tag_version: str) -> str:
    """Validate workspace and locked local package versions."""
    root_manifest = load_toml(repo_root / "Cargo.toml")
    workspace = root_manifest.get("workspace")
    if not isinstance(workspace, dict):
        raise ValidationError("Cargo.toml is missing [workspace]")
    workspace_package = workspace.get("package")
    if not isinstance(workspace_package, dict):
        raise ValidationError("Cargo.toml is missing [workspace.package]")

    version = workspace_package.get("version")
    if not isinstance(version, str) or STABLE_VERSION_PATTERN.fullmatch(version) is None:
        raise ValidationError(
            "Cargo workspace version must be stable X.Y.Z SemVer without leading zeroes"
        )
    if version != tag_version:
        raise ValidationError(f"tag version {tag_version} does not match Cargo version {version}")

    member_names: set[str] = set()
    for manifest_path in workspace_member_manifests(repo_root, workspace):
        manifest = load_toml(manifest_path)
        package = manifest.get("package")
        if not isinstance(package, dict):
            raise ValidationError(f"{manifest_path} is missing [package]")
        name = package.get("name")
        if not isinstance(name, str) or not name:
            raise ValidationError(f"{manifest_path} has no package name")
        if name in member_names:
            raise ValidationError(f"duplicate Cargo workspace package name: {name}")
        member_names.add(name)

        member_version = package.get("version")
        if isinstance(member_version, dict) and member_version.get("workspace") is True:
            resolved_version = version
        elif isinstance(member_version, str):
            resolved_version = member_version
        else:
            raise ValidationError(f"{manifest_path} has no resolvable package version")
        if resolved_version != version:
            raise ValidationError(
                f"Cargo workspace package {name} has version {resolved_version}, expected {version}"
            )

    lockfile = load_toml(repo_root / "Cargo.lock")
    locked_packages = lockfile.get("package")
    if not isinstance(locked_packages, list):
        raise ValidationError("Cargo.lock is missing package entries")
    for name in sorted(member_names):
        candidates = [
            package
            for package in locked_packages
            if isinstance(package, dict)
            and package.get("name") == name
            and "source" not in package
        ]
        if len(candidates) != 1:
            raise ValidationError(
                f"Cargo.lock must contain exactly one local workspace package named {name}"
            )
        locked_version = candidates[0].get("version")
        if locked_version != version:
            raise ValidationError(
                f"Cargo.lock workspace package {name} has version {locked_version}, expected {version}"
            )

    return version


def validate_sparkle_pin(repo_root: Path) -> str:
    """Validate that SwiftPM uses one exact Sparkle version and matching pin."""
    package_path = repo_root / "native/macos-host/Package.swift"
    package_source = package_path.read_text(encoding="utf-8")
    sparkle_mentions = package_source.count("github.com/sparkle-project/Sparkle")
    exact_matches = SPARKLE_EXACT_PATTERN.findall(package_source)
    if sparkle_mentions != 1 or len(exact_matches) != 1:
        raise ValidationError(
            "Package.swift must declare Sparkle exactly once with an exact version"
        )
    sparkle_version = exact_matches[0]
    if STABLE_VERSION_PATTERN.fullmatch(sparkle_version) is None:
        raise ValidationError(
            "Package.swift Sparkle exact version must be stable X.Y.Z SemVer "
            "without leading zeroes"
        )

    resolved_path = repo_root / "native/macos-host/Package.resolved"
    try:
        resolved = json.loads(resolved_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValidationError(f"cannot read Package.resolved: {error}") from error
    pins = resolved.get("pins") if isinstance(resolved, dict) else None
    if not isinstance(pins, list):
        raise ValidationError("Package.resolved is missing pins")
    sparkle_pins = [
        pin
        for pin in pins
        if isinstance(pin, dict) and str(pin.get("identity", "")).lower() == "sparkle"
    ]
    if len(sparkle_pins) != 1:
        raise ValidationError("Package.resolved must contain exactly one Sparkle pin")

    sparkle_pin = sparkle_pins[0]
    location = str(sparkle_pin.get("location", "")).removesuffix(".git")
    if location != SPARKLE_LOCATION:
        raise ValidationError(f"Package.resolved Sparkle location is not {SPARKLE_LOCATION}")
    state = sparkle_pin.get("state")
    if not isinstance(state, dict):
        raise ValidationError("Package.resolved Sparkle pin has no state")
    if state.get("version") != sparkle_version:
        raise ValidationError(
            "Package.resolved Sparkle version does not match Package.swift exact version"
        )
    revision = state.get("revision")
    if not isinstance(revision, str) or re.fullmatch(r"[0-9a-fA-F]{40,64}", revision) is None:
        raise ValidationError("Package.resolved Sparkle pin has no full revision")

    return sparkle_version


def write_outputs(version: str, tag_commit: str, sparkle_version: str) -> None:
    """Write validated values to the GitHub Actions output file."""
    output_path = Path(require_environment("GITHUB_OUTPUT"))
    with output_path.open("a", encoding="utf-8") as stream:
        stream.write(f"version={version}\n")
        stream.write(f"tag_commit={tag_commit}\n")
        stream.write(f"sparkle_version={sparkle_version}\n")


def main() -> int:
    """Validate release source state and publish trusted outputs."""
    tag = require_environment("GITHUB_REF_NAME")
    tag_match = STABLE_TAG_PATTERN.fullmatch(tag)
    if tag_match is None:
        raise ValidationError(
            "release tag must be stable vX.Y.Z SemVer without leading zeroes"
        )
    tag_version = tag_match.group(1)

    repo_root = Path(
        subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    )
    _, event_after = validate_github_event(tag)
    tag_commit = validate_tag_and_source(repo_root, tag, event_after)
    version = validate_cargo_versions(repo_root, tag_version)
    sparkle_version = validate_sparkle_pin(repo_root)
    write_outputs(version, tag_commit, sparkle_version)
    print(
        f"validated release source: version={version} "
        f"tag_commit={tag_commit} sparkle_version={sparkle_version}"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, subprocess.CalledProcessError, tomllib.TOMLDecodeError, ValidationError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
