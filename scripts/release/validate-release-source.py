#!/usr/bin/env python3
"""Validate the immutable source inputs for an Rsnap release tag."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


CANONICAL_REPOSITORY = "acg-box/rsnap"
SEMVER_COMPONENT = r"(?:0|[1-9][0-9]*)"
STABLE_VERSION_RE = re.compile(
	rf"^{SEMVER_COMPONENT}\.{SEMVER_COMPONENT}\.{SEMVER_COMPONENT}$"
)
STABLE_TAG_RE = re.compile(
	rf"^v({SEMVER_COMPONENT}\.{SEMVER_COMPONENT}\.{SEMVER_COMPONENT})$"
)
SPARKLE_DECLARATION_RE = re.compile(
	r'\.package\(url:\s*"https://github\.com/sparkle-project/Sparkle",\s*'
	r'exact:\s*"([^"]+)"\)'
)
RSNAP_PACKAGES = {"rsnap", "rsnap-perf", "rsnap-capture-core", "rsnap-host-ffi"}


class ValidationError(RuntimeError):
	"""A release source contract failed."""


def parse_args() -> argparse.Namespace:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--tag", required=True)
	parser.add_argument("--event-object", required=True)
	parser.add_argument("--repository", required=True)
	parser.add_argument("--base-ref", default="refs/remotes/origin/main")
	parser.add_argument(
		"--repo-root",
		type=Path,
		default=Path(__file__).resolve().parents[2],
	)
	parser.add_argument("--github-output", type=Path)
	return parser.parse_args()


def fail(message: str) -> None:
	raise ValidationError(message)


def git(repo_root: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
	return subprocess.run(
		["git", "-C", str(repo_root), *args],
		check=check,
		capture_output=True,
		text=True,
	)


def git_output(repo_root: Path, *args: str) -> str:
	try:
		return git(repo_root, *args).stdout.strip()
	except subprocess.CalledProcessError as error:
		detail = error.stderr.strip() or error.stdout.strip() or "git command failed"
		fail(f"{detail}: git {' '.join(args)}")
	return ""


def read_text(path: Path) -> str:
	try:
		return path.read_text(encoding="utf-8")
	except OSError as error:
		fail(f"cannot read {path}: {error}")
	return ""


def toml_basic_string(section: str, key: str, *, source: str) -> str:
	matches = re.findall(
		rf'^\s*{re.escape(key)}\s*=\s*("(?:[^"\\]|\\.)*")\s*(?:#.*)?$',
		section,
		re.MULTILINE,
	)
	if len(matches) != 1:
		fail(f"{source} must contain exactly one string value for {key}")
	try:
		value = json.loads(matches[0])
	except json.JSONDecodeError as error:
		fail(f"{source} contains an invalid string value for {key}: {error}")
	if not isinstance(value, str):
		fail(f"{source} {key} must be a string")
	return value


def named_toml_section(document: str, name: str, *, source: str) -> str:
	section_matches = list(
		re.finditer(
			rf"^\s*\[{re.escape(name)}\]\s*(?:#.*)?$",
			document,
			re.MULTILINE,
		)
	)
	if len(section_matches) != 1:
		fail(f"{source} must contain exactly one [{name}] section")
	start = section_matches[0].end()
	next_section = re.search(r"^\s*\[", document[start:], re.MULTILINE)
	end = start + next_section.start() if next_section is not None else len(document)
	return document[start:end]


def validate_versions(repo_root: Path, tag: str) -> tuple[str, str]:
	tag_match = STABLE_TAG_RE.fullmatch(tag)
	if tag_match is None:
		fail(f"release tag must be stable SemVer vX.Y.Z without leading zeroes: {tag}")
	version = tag_match.group(1)

	cargo_text = read_text(repo_root / "Cargo.toml")
	cargo_package = named_toml_section(
		cargo_text,
		"workspace.package",
		source="Cargo.toml",
	)
	cargo_version = toml_basic_string(cargo_package, "version", source="Cargo.toml")
	if not isinstance(cargo_version, str) or STABLE_VERSION_RE.fullmatch(cargo_version) is None:
		fail(f"Cargo workspace version is not stable SemVer: {cargo_version!r}")
	if cargo_version != version:
		fail(f"tag {tag} does not match Cargo workspace version {cargo_version}")

	lock_text = read_text(repo_root / "Cargo.lock")
	package_sections = re.split(r"^\s*\[\[package\]\]\s*(?:#.*)?$", lock_text, flags=re.MULTILINE)[1:]
	locked_versions: dict[str, list[str]] = {name: [] for name in RSNAP_PACKAGES}
	for package_section in package_sections:
		name_matches = re.findall(
			r'^\s*name\s*=\s*("(?:[^"\\]|\\.)*")\s*(?:#.*)?$',
			package_section,
			re.MULTILINE,
		)
		if len(name_matches) != 1:
			continue
		try:
			package_name = json.loads(name_matches[0])
		except json.JSONDecodeError:
			continue
		if package_name in locked_versions:
			locked_versions[package_name].append(
				toml_basic_string(package_section, "version", source=f"Cargo.lock {package_name}")
			)
	missing_packages = {name for name, versions in locked_versions.items() if not versions}
	if missing_packages:
		missing = ", ".join(sorted(missing_packages))
		fail(f"Cargo.lock is missing Rsnap workspace packages: {missing}")
	for package, versions in sorted(locked_versions.items()):
		if versions != [version]:
			fail(f"Cargo.lock {package} versions {versions!r} do not match exactly {version}")

	package_swift_path = repo_root / "native/macos-host/Package.swift"
	try:
		package_swift = package_swift_path.read_text(encoding="utf-8")
	except OSError as error:
		fail(f"cannot read {package_swift_path}: {error}")
	declared_sparkle_versions = SPARKLE_DECLARATION_RE.findall(package_swift)
	if len(declared_sparkle_versions) != 1:
		fail("Package.swift must declare exactly one exact official Sparkle dependency")
	sparkle_version = declared_sparkle_versions[0]
	if STABLE_VERSION_RE.fullmatch(sparkle_version) is None:
		fail(f"Package.swift Sparkle version is not stable SemVer: {sparkle_version}")

	resolved_path = repo_root / "native/macos-host/Package.resolved"
	try:
		resolved = json.loads(resolved_path.read_text(encoding="utf-8"))
	except (OSError, json.JSONDecodeError) as error:
		fail(f"cannot read {resolved_path}: {error}")
	sparkle_pins = [
		pin
		for pin in resolved.get("pins", [])
		if isinstance(pin, dict) and pin.get("identity") == "sparkle"
	]
	if len(sparkle_pins) != 1:
		fail("Package.resolved must contain exactly one Sparkle pin")
	pin = sparkle_pins[0]
	if pin.get("location") != "https://github.com/sparkle-project/Sparkle":
		fail(f"Package.resolved uses an unexpected Sparkle source: {pin.get('location')!r}")
	state = pin.get("state")
	if not isinstance(state, dict):
		fail("Package.resolved Sparkle pin is missing state")
	if state.get("version") != sparkle_version:
		fail(
			"Package.swift and Package.resolved disagree on Sparkle version: "
			f"{sparkle_version} != {state.get('version')!r}"
		)
	revision = state.get("revision")
	if not isinstance(revision, str) or re.fullmatch(r"[0-9a-f]{40}", revision) is None:
		fail("Package.resolved Sparkle pin must contain a full lowercase commit SHA")

	return version, sparkle_version


def validate_git_source(
	repo_root: Path,
	tag: str,
	event_object: str,
	base_ref: str,
) -> str:
	if re.fullmatch(r"[0-9a-fA-F]{40}", event_object) is None:
		fail(f"GitHub event object must be a full Git object SHA: {event_object}")
	if not base_ref.startswith("refs/remotes/origin/"):
		fail(f"release base must be an origin remote-tracking ref: {base_ref}")

	tag_ref = f"refs/tags/{tag}"
	if git_output(repo_root, "cat-file", "-t", tag_ref) != "tag":
		fail(f"release tag must be an annotated tag: {tag}")
	tag_headers = git_output(repo_root, "cat-file", "-p", tag_ref).partition("\n\n")[0]
	direct_target_type = re.search(r"^type (\S+)$", tag_headers, re.MULTILINE)
	declared_tag_name = re.search(r"^tag (.+)$", tag_headers, re.MULTILINE)
	if direct_target_type is None or direct_target_type.group(1) != "commit":
		fail(f"release annotated tag must point directly to a commit: {tag}")
	if declared_tag_name is None or declared_tag_name.group(1) != tag:
		fail(f"release annotated tag object name must match its ref: {tag}")
	tag_commit = git_output(repo_root, "rev-parse", "--verify", f"{tag_ref}^{{commit}}")
	event_commit = git_output(
		repo_root,
		"rev-parse",
		"--verify",
		f"{event_object}^{{commit}}",
	)
	head_commit = git_output(repo_root, "rev-parse", "--verify", "HEAD^{commit}")
	git_output(repo_root, "rev-parse", "--verify", f"{base_ref}^{{commit}}")

	if tag_commit != event_commit:
		fail(f"tag commit {tag_commit} does not match event commit {event_commit}")
	if tag_commit != head_commit:
		fail(f"checked-out commit {head_commit} does not match tag commit {tag_commit}")
	ancestor = git(repo_root, "merge-base", "--is-ancestor", tag_commit, base_ref, check=False)
	if ancestor.returncode != 0:
		fail(f"tag commit {tag_commit} is not reachable from {base_ref}")
	return tag_commit


def emit_outputs(
	output_path: Path | None,
	*,
	version: str,
	sparkle_version: str,
	tag_commit: str,
) -> None:
	values = {
		"canonical_repository": CANONICAL_REPOSITORY,
		"sparkle_version": sparkle_version,
		"tag_commit": tag_commit,
		"version": version,
	}
	if output_path is None:
		print(json.dumps(values, sort_keys=True))
		return
	with output_path.open("a", encoding="utf-8") as handle:
		for key, value in values.items():
			handle.write(f"{key}={value}\n")


def main() -> int:
	args = parse_args()
	repo_root = args.repo_root.resolve()
	if args.repository != CANONICAL_REPOSITORY:
		fail(
			f"release repository must be {CANONICAL_REPOSITORY}, got {args.repository}"
		)
	version, sparkle_version = validate_versions(repo_root, args.tag)
	tag_commit = validate_git_source(
		repo_root,
		args.tag,
		args.event_object,
		args.base_ref,
	)
	emit_outputs(
		args.github_output,
		version=version,
		sparkle_version=sparkle_version,
		tag_commit=tag_commit,
	)
	return 0


if __name__ == "__main__":
	try:
		raise SystemExit(main())
	except ValidationError as error:
		print(f"error: {error}", file=sys.stderr)
		raise SystemExit(1) from error
