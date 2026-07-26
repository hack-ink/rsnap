#!/usr/bin/env python3
"""Validate that an Rsnap release cannot regress the public Sparkle latest channel."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


SEMVER_COMPONENT = r"(?:0|[1-9][0-9]*)"
STABLE_VERSION_RE = re.compile(
	rf"^({SEMVER_COMPONENT})\.({SEMVER_COMPONENT})\.({SEMVER_COMPONENT})$"
)
STABLE_TAG_RE = re.compile(
	rf"^v({SEMVER_COMPONENT})\.({SEMVER_COMPONENT})\.({SEMVER_COMPONENT})$"
)


class ValidationError(RuntimeError):
	"""The GitHub release order is unsafe or cannot be proved."""


def parse_args() -> argparse.Namespace:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--releases-json", type=Path, required=True)
	parser.add_argument("--tag", required=True)
	parser.add_argument("--version", required=True)
	parser.add_argument("--inventory-limit", type=int, default=1000)
	return parser.parse_args()


def fail(message: str) -> None:
	raise ValidationError(message)


def stable_version(value: str, *, source: str) -> tuple[int, int, int]:
	match = STABLE_VERSION_RE.fullmatch(value)
	if match is None:
		fail(f"{source} must be stable SemVer without leading zeroes: {value!r}")
	return tuple(int(component) for component in match.groups())  # type: ignore[return-value]


def stable_tag(value: str) -> tuple[int, int, int] | None:
	match = STABLE_TAG_RE.fullmatch(value)
	if match is None:
		return None
	return tuple(int(component) for component in match.groups())  # type: ignore[return-value]


def load_releases(path: Path) -> list[dict[str, Any]]:
	try:
		value = json.loads(path.read_text(encoding="utf-8"))
	except (OSError, json.JSONDecodeError) as error:
		fail(f"cannot read GitHub release inventory: {error}")
	if not isinstance(value, list):
		fail("GitHub release inventory must be a JSON array")
	releases: list[dict[str, Any]] = []
	for index, item in enumerate(value):
		if not isinstance(item, dict):
			fail(f"GitHub release inventory item {index} must be an object")
		for field in ("tagName", "isDraft", "isPrerelease", "isLatest"):
			if field not in item:
				fail(f"GitHub release inventory item {index} is missing {field}")
		if not isinstance(item["tagName"], str) or not item["tagName"]:
			fail(f"GitHub release inventory item {index} has an invalid tagName")
		for field in ("isDraft", "isPrerelease", "isLatest"):
			if not isinstance(item[field], bool):
				fail(f"GitHub release inventory item {index} has a non-boolean {field}")
		releases.append(item)
	return releases


def validate_release_order(
	releases: list[dict[str, Any]],
	*,
	tag: str,
	version: str,
	inventory_limit: int,
) -> str:
	if inventory_limit <= 0:
		fail("release inventory limit must be positive")
	if len(releases) >= inventory_limit:
		fail("GitHub release inventory reached its limit and may be incomplete")

	tag_version = stable_tag(tag)
	version_tuple = stable_version(version, source="release version")
	if tag_version is None or tag_version != version_tuple:
		fail("release tag and version must describe the same stable SemVer")

	latest_releases = [release for release in releases if release["isLatest"]]
	published_stable: list[tuple[tuple[int, int, int], dict[str, Any]]] = []
	for release in releases:
		release_version = stable_tag(release["tagName"])
		if (
			release_version is not None
			and release["isDraft"] is False
			and release["isPrerelease"] is False
		):
			published_stable.append((release_version, release))

	if published_stable:
		if len(latest_releases) != 1:
			fail("GitHub must identify exactly one latest release")
		max_version, max_release = max(published_stable, key=lambda item: item[0])
		latest_release = latest_releases[0]
		if latest_release is not max_release:
			fail(
				"GitHub latest release must be the highest published stable SemVer: "
				f"expected {max_release['tagName']}, got {latest_release['tagName']}"
			)
	else:
		max_version = None
		max_release = None
		if latest_releases:
			fail("GitHub latest release must use a stable SemVer tag")

	target_releases = [release for release in releases if release["tagName"] == tag]
	if len(target_releases) > 1:
		fail(f"GitHub release inventory contains duplicate releases for {tag}")
	if not target_releases:
		target_state = "absent"
	else:
		target = target_releases[0]
		if target["isPrerelease"]:
			fail("stable Rsnap release must not be marked as a prerelease")
		target_state = "draft" if target["isDraft"] else "published"

	if (
		target_state != "published"
		and max_version is not None
		and version_tuple <= max_version
	):
		assert max_release is not None
		fail(
			f"release {tag} must be newer than current latest stable release "
			f"{max_release['tagName']}"
		)
	return target_state


def main() -> int:
	args = parse_args()
	releases = load_releases(args.releases_json)
	state = validate_release_order(
		releases,
		tag=args.tag,
		version=args.version,
		inventory_limit=args.inventory_limit,
	)
	print(state)
	return 0


if __name__ == "__main__":
	try:
		raise SystemExit(main())
	except ValidationError as error:
		print(f"error: {error}", file=sys.stderr)
		raise SystemExit(1) from error
