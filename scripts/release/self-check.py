#!/usr/bin/env python3
"""Run credential-free release workflow regression checks."""

from __future__ import annotations

import base64
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


ROOT = Path(__file__).resolve().parents[2]
RELEASE_DIR = ROOT / "scripts/release"
CANONICAL_REPOSITORY = "acg-box/rsnap"
ARCHIVE_NAME = "rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME = "appcast.xml"
CHECKSUM_NAME = f"{ARCHIVE_NAME}.sha256"
PUBLIC_KEY = "X2EaTv6mCzkYxz75Hh+ldMkKlpzNlHRg5l7Kn9ke8Ow="
SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"


def run(
	args: list[str | Path],
	*,
	cwd: Path | None = None,
	env: dict[str, str] | None = None,
	input_text: str | None = None,
	check: bool = True,
) -> subprocess.CompletedProcess[str]:
	command = [str(arg) for arg in args]
	result = subprocess.run(
		command,
		cwd=cwd,
		env=env,
		input=input_text,
		capture_output=True,
		text=True,
	)
	if check and result.returncode != 0:
		raise AssertionError(
			f"command failed ({result.returncode}): {' '.join(command)}\n"
			f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
		)
	return result


def expect_failure(
	args: list[str | Path],
	*,
	cwd: Path | None = None,
	env: dict[str, str] | None = None,
	input_text: str | None = None,
) -> None:
	result = run(args, cwd=cwd, env=env, input_text=input_text, check=False)
	if result.returncode == 0:
		raise AssertionError(f"command unexpectedly succeeded: {' '.join(map(str, args))}")


def write_executable(path: Path, contents: str) -> None:
	path.write_text(contents, encoding="utf-8")
	path.chmod(0o755)


def git(repo: Path, *args: str) -> str:
	return run(["git", *args], cwd=repo).stdout.strip()


def test_source_validator(tmp: Path) -> None:
	repo = tmp / "source"
	(repo / "native/macos-host").mkdir(parents=True)
	(repo / "Cargo.toml").write_text(
		"""\
[workspace]
members = ["apps/rsnap", "apps/rsnap-perf", "packages/rsnap-capture-core", "packages/rsnap-host-ffi"]

[workspace.package]
version = "1.2.3"
""",
		encoding="utf-8",
	)
	(repo / "Cargo.lock").write_text(
		"""\
version = 4

[[package]]
name = "rsnap"
version = "1.2.3"

[[package]]
name = "rsnap-perf"
version = "1.2.3"

[[package]]
name = "rsnap-capture-core"
version = "1.2.3"

[[package]]
name = "rsnap-host-ffi"
version = "1.2.3"
""",
		encoding="utf-8",
	)
	(repo / "native/macos-host/Package.swift").write_text(
		'.package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.9.4")\n',
		encoding="utf-8",
	)
	resolved_path = repo / "native/macos-host/Package.resolved"
	resolved = {
		"pins": [
			{
				"identity": "sparkle",
				"location": "https://github.com/sparkle-project/Sparkle",
				"state": {"revision": "b" * 40, "version": "2.9.4"},
			}
		]
	}
	resolved_path.write_text(json.dumps(resolved), encoding="utf-8")

	git(repo, "init", "-b", "main")
	git(repo, "config", "user.name", "Release Self Check")
	git(repo, "config", "user.email", "release-self-check@example.invalid")
	git(repo, "config", "core.hooksPath", str(repo / ".git/hooks"))
	git(repo, "add", ".")
	git(repo, "commit", "-m", "base")
	base_commit = git(repo, "rev-parse", "HEAD")
	git(repo, "update-ref", "refs/remotes/origin/main", base_commit)
	git(repo, "commit", "--allow-empty", "-m", "tagged feature")
	tag_commit = git(repo, "rev-parse", "HEAD")
	git(repo, "tag", "-a", "v1.2.3", "-m", "v1.2.3")
	tag_object = git(repo, "rev-parse", "refs/tags/v1.2.3")

	validator = RELEASE_DIR / "validate-release-source.py"
	base_args = [
		validator,
		"--repo-root",
		repo,
		"--tag",
		"v1.2.3",
		"--event-object",
		tag_object,
		"--repository",
		CANONICAL_REPOSITORY,
	]
	expect_failure(base_args)
	git(repo, "update-ref", "refs/remotes/origin/main", tag_commit)
	result = run(base_args)
	metadata = json.loads(result.stdout)
	assert metadata["version"] == "1.2.3"
	assert metadata["sparkle_version"] == "2.9.4"
	assert metadata["tag_commit"] == tag_commit

	wrong_repository_args = list(base_args)
	wrong_repository_args[-1] = "other/rsnap"
	expect_failure(wrong_repository_args)
	wrong_event_args = list(base_args)
	wrong_event_args[wrong_event_args.index("--event-object") + 1] = base_commit
	expect_failure(wrong_event_args)
	leading_zero_args = list(base_args)
	leading_zero_args[leading_zero_args.index("--tag") + 1] = "v01.2.3"
	expect_failure(leading_zero_args)

	bad_resolved = json.loads(json.dumps(resolved))
	bad_resolved["pins"][0]["state"]["version"] = "2.9.3"
	resolved_path.write_text(json.dumps(bad_resolved), encoding="utf-8")
	expect_failure(base_args)
	resolved_path.write_text(json.dumps(resolved), encoding="utf-8")

	git(repo, "tag", "-d", "v1.2.3")
	git(repo, "tag", "v1.2.3")
	lightweight_object = git(repo, "rev-parse", "refs/tags/v1.2.3")
	lightweight_args = list(base_args)
	lightweight_args[lightweight_args.index("--event-object") + 1] = lightweight_object
	expect_failure(lightweight_args)

	git(repo, "tag", "-d", "v1.2.3")
	git(repo, "tag", "-a", "nested-target", "-m", "nested-target")
	git(repo, "tag", "-a", "v1.2.3", "-m", "v1.2.3", "nested-target")
	nested_object = git(repo, "rev-parse", "refs/tags/v1.2.3")
	nested_args = list(base_args)
	nested_args[nested_args.index("--event-object") + 1] = nested_object
	expect_failure(nested_args)


def create_signing_fixture(root: Path) -> Path:
	app = root / "Rsnap.app"
	version_root = app / "Contents/Frameworks/Sparkle.framework/Versions/B"
	(version_root / "XPCServices/Installer.xpc").mkdir(parents=True)
	(version_root / "XPCServices/Downloader.xpc").mkdir(parents=True)
	(version_root / "Updater.app").mkdir(parents=True)
	autoupdate = version_root / "Autoupdate"
	autoupdate.write_bytes(b"mach-o")
	autoupdate.chmod(0o755)
	(version_root / "Sparkle").write_bytes(b"mach-o")
	(app / "Contents/Frameworks/Sparkle.framework/Versions/Current").symlink_to("B")
	return app


def test_signer(tmp: Path) -> None:
	fixture_root = tmp / "signing"
	fixture_root.mkdir()
	app = create_signing_fixture(fixture_root)
	keychain = fixture_root / "release.keychain-db"
	keychain.touch()
	log_path = fixture_root / "codesign.jsonl"
	fake_codesign = fixture_root / "codesign"
	write_executable(
		fake_codesign,
		"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
with Path(os.environ["CODESIGN_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(args) + "\\n")
if "-dv" in args:
    lines = [
        "Executable=fake",
        "Identifier=fake",
        "CodeDirectory v=20500 size=1 flags=0x10000(runtime) hashes=1+0 location=embedded",
        "Authority=Developer ID Application: Rsnap Release (TEAM123)",
        "TeamIdentifier=TEAM123",
    ]
    if os.environ.get("FAKE_CODESIGN_DETAILS") != "missing_timestamp":
        lines.append("Timestamp=Jul 26, 2026 at 1:00:00 PM")
    print("\\n".join(lines), file=sys.stderr)
elif "--entitlements" in args and "-d" in args:
    print("<plist><dict></dict></plist>")
""",
	)
	env = os.environ.copy()
	env.update({"CODESIGN_LOG": str(log_path), "RSNAP_CODESIGN_BIN": str(fake_codesign)})
	identity = "Developer ID Application: Rsnap Release (TEAM123)"
	run(
		[
			RELEASE_DIR / "sign-macos-app.sh",
			"--app",
			app,
			"--identity",
			identity,
			"--keychain",
			keychain,
			"--mode",
			"release",
		],
		env=env,
	)
	calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	sign_calls = [call for call in calls if "--force" in call]
	expected_suffixes = [
		"Versions/B/XPCServices/Installer.xpc",
		"Versions/B/XPCServices/Downloader.xpc",
		"Versions/B/Autoupdate",
		"Versions/B/Updater.app",
		"Sparkle.framework",
		"Rsnap.app",
	]
	assert len(sign_calls) == len(expected_suffixes)
	for call, suffix in zip(sign_calls, expected_suffixes):
		assert call[-1].endswith(suffix), (call, suffix)
		assert "--deep" not in call
		assert call.index("--sign") < call.index("--options")
		assert call[call.index("--options") + 1] == "runtime"
		assert "--timestamp" in call
	assert "--preserve-metadata=entitlements" not in sign_calls[0]
	assert "--preserve-metadata=entitlements" in sign_calls[1]
	assert all("--preserve-metadata=entitlements" not in call for call in sign_calls[2:])
	assert any("--deep" in call and "--verify" in call for call in calls)

	invalid_env = dict(env)
	invalid_env["FAKE_CODESIGN_DETAILS"] = "missing_timestamp"
	expect_failure(
		[
			RELEASE_DIR / "sign-macos-app.sh",
			"--app",
			app,
			"--identity",
			identity,
			"--keychain",
			keychain,
			"--mode",
			"release",
		],
		env=invalid_env,
	)


def test_appcast(tmp: Path) -> None:
	fixture_root = tmp / "appcast"
	fixture_root.mkdir()
	archive = fixture_root / ARCHIVE_NAME
	archive.write_bytes(b"final-zip-bytes")
	appcast = fixture_root / APPCAST_NAME
	log_path = fixture_root / "sign-update.jsonl"
	sign_update = fixture_root / "sign_update"
	write_executable(
		sign_update,
		"""#!/usr/bin/env python3
import base64
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
with Path(os.environ["SIGN_UPDATE_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(args) + "\\n")
sys.stdin.read()
if "--verify" not in args:
    print(base64.b64encode(bytes(64)).decode("ascii"))
""",
	)
	env = os.environ.copy()
	env.update(
		{
			"RSNAP_SPARKLE_PRIVATE_ED_KEY": "fixture-private-key",
			"SIGN_UPDATE_LOG": str(log_path),
			"SPARKLE_SIGN_UPDATE": str(sign_update),
		}
	)
	run(
		[
			RELEASE_DIR / "sparkle-appcast.sh",
			"--archive",
			archive,
			"--appcast",
			appcast,
			"--version",
			"1.2.3",
			"--tag",
			"v1.2.3",
		],
		env=env,
	)
	root = ET.parse(appcast).getroot()
	item = root.find("./channel/item")
	assert item is not None
	assert item.findtext(f"{{{SPARKLE_NAMESPACE}}}minimumSystemVersion") == "14.0.0"
	enclosure = item.find("enclosure")
	assert enclosure is not None
	assert enclosure.get("length") == str(archive.stat().st_size)
	assert enclosure.get("url") == (
		f"https://github.com/{CANONICAL_REPOSITORY}/releases/download/v1.2.3/{ARCHIVE_NAME}"
	)
	calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	assert len(calls) == 2
	assert "-p" in calls[0]
	assert "--verify" in calls[1]

	wrong_env = dict(env)
	wrong_env.pop("RSNAP_SPARKLE_PRIVATE_ED_KEY")
	wrong_env["SPARKLE_PRIVATE_ED_KEY"] = "forbidden-generic-key"
	expect_failure(
		[
			RELEASE_DIR / "sparkle-appcast.sh",
			"--archive",
			archive,
			"--appcast",
			appcast,
			"--version",
			"1.2.3",
			"--tag",
			"v1.2.3",
		],
		env=wrong_env,
	)


def test_sparkle_key_verifier(tmp: Path) -> None:
	if sys.platform != "darwin":
		return
	swift = shutil.which("swift")
	if swift is None:
		raise AssertionError("swift is required for the macOS Sparkle key self-check")
	generator = tmp / "generate-sparkle-key.swift"
	generator.write_text(
		"""\
import CryptoKit
import Foundation

let key = Curve25519.Signing.PrivateKey()
print(key.rawRepresentation.base64EncodedString())
print(key.publicKey.rawRepresentation.base64EncodedString())
""",
		encoding="utf-8",
	)
	generated = run([swift, generator]).stdout.splitlines()
	assert len(generated) == 2
	private_key, public_key = generated
	verifier = RELEASE_DIR / "verify-sparkle-key.swift"
	run([verifier, public_key], input_text=f"{private_key}\n")
	wrong_public_key = base64.b64encode(bytes(32)).decode("ascii")
	expect_failure([verifier, wrong_public_key], input_text=f"{private_key}\n")


def write_artifact_fixture(root: Path) -> tuple[Path, Path, Path, Path, Path]:
	archive = root / ARCHIVE_NAME
	appcast = root / APPCAST_NAME
	checksum = root / CHECKSUM_NAME
	main_plist = plistlib.dumps(
		{
			"CFBundleName": "Rsnap",
			"CFBundleDisplayName": "Rsnap",
			"CFBundleIdentifier": "ink.hack.rsnap",
			"CFBundleShortVersionString": "1.2.3",
			"CFBundleVersion": "1.2.3",
			"LSMinimumSystemVersion": "14.0",
			"SUFeedURL": (
				f"https://github.com/{CANONICAL_REPOSITORY}/releases/latest/download/{APPCAST_NAME}"
			),
			"SUPublicEDKey": PUBLIC_KEY,
		}
	)
	framework_plist = plistlib.dumps(
		{
			"CFBundleIdentifier": "org.sparkle-project.Sparkle",
			"CFBundleShortVersionString": "2.9.4",
		}
	)
	with zipfile.ZipFile(archive, "w") as bundle:
		bundle.writestr("Rsnap.app/Contents/Info.plist", main_plist)
		bundle.writestr("Rsnap.app/Contents/MacOS/RsnapNativeHost", b"mach-o")
		for relative_path in (
			"Sparkle",
			"Autoupdate",
			"Updater.app/Contents/Info.plist",
			"XPCServices/Installer.xpc/Contents/Info.plist",
			"XPCServices/Downloader.xpc/Contents/Info.plist",
		):
			bundle.writestr(
				"Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/B/"
				+ relative_path,
				b"fixture",
			)
		bundle.writestr(
			"Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/B/Resources/Info.plist",
			framework_plist,
		)

	signature = base64.b64encode(bytes(64)).decode("ascii")
	rss = ET.Element("rss", {"version": "2.0"})
	channel = ET.SubElement(rss, "channel")
	item = ET.SubElement(channel, "item")
	release_url = f"https://github.com/{CANONICAL_REPOSITORY}/releases/tag/v1.2.3"
	ET.SubElement(item, "link").text = release_url
	ET.SubElement(item, f"{{{SPARKLE_NAMESPACE}}}version").text = "1.2.3"
	ET.SubElement(item, f"{{{SPARKLE_NAMESPACE}}}shortVersionString").text = "1.2.3"
	ET.SubElement(item, f"{{{SPARKLE_NAMESPACE}}}minimumSystemVersion").text = "14.0.0"
	ET.SubElement(item, f"{{{SPARKLE_NAMESPACE}}}hardwareRequirements").text = "arm64"
	ET.SubElement(item, f"{{{SPARKLE_NAMESPACE}}}releaseNotesLink").text = release_url
	ET.SubElement(
		item,
		"enclosure",
		{
			"url": (
				f"https://github.com/{CANONICAL_REPOSITORY}/releases/download/"
				f"v1.2.3/{ARCHIVE_NAME}"
			),
			f"{{{SPARKLE_NAMESPACE}}}edSignature": signature,
			"length": str(archive.stat().st_size),
			"type": "application/octet-stream",
		},
	)
	ET.ElementTree(rss).write(appcast, encoding="utf-8", xml_declaration=True)
	archive_digest = hashlib.sha256(archive.read_bytes()).hexdigest()
	checksum.write_text(f"{archive_digest}  {ARCHIVE_NAME}\n", encoding="utf-8")

	local_assets = {path.name: path for path in (archive, appcast, checksum)}
	release_json = root / "release.json"
	assets_json = root / "assets.json"
	draft_slug = "untagged-fixture-123"
	release_json.write_text(
		json.dumps(
			{
				"id": 123,
				"tag_name": "v1.2.3",
				"draft": True,
				"prerelease": False,
				"html_url": (
					f"https://github.com/{CANONICAL_REPOSITORY}/releases/tag/{draft_slug}"
				),
			}
		),
		encoding="utf-8",
	)
	assets_json.write_text(
		json.dumps(
			[
				{
					"name": name,
					"state": "uploaded",
					"size": path.stat().st_size,
					"browser_download_url": (
						f"https://github.com/{CANONICAL_REPOSITORY}/releases/download/"
						f"{draft_slug}/{name}"
					),
					"digest": f"sha256:{hashlib.sha256(path.read_bytes()).hexdigest()}",
				}
				for name, path in local_assets.items()
			]
		),
		encoding="utf-8",
	)
	return archive, appcast, checksum, release_json, assets_json


def test_artifact_validator(tmp: Path) -> None:
	fixture_root = tmp / "artifacts"
	fixture_root.mkdir()
	archive, appcast, checksum, release_json, assets_json = write_artifact_fixture(fixture_root)
	validator = RELEASE_DIR / "validate-release-artifacts.py"
	args = [
		validator,
		"--archive",
		archive,
		"--appcast",
		appcast,
		"--checksum",
		checksum,
		"--release-json",
		release_json,
		"--assets-json",
		assets_json,
		"--version",
		"1.2.3",
		"--sparkle-version",
		"2.9.4",
		"--tag",
		"v1.2.3",
		"--repository",
		CANONICAL_REPOSITORY,
		"--release-state",
		"draft",
	]
	run(args)
	published_release = json.loads(release_json.read_text(encoding="utf-8"))
	published_release["draft"] = False
	published_release["html_url"] = (
		f"https://github.com/{CANONICAL_REPOSITORY}/releases/tag/v1.2.3"
	)
	release_json.write_text(json.dumps(published_release), encoding="utf-8")
	published_assets = json.loads(assets_json.read_text(encoding="utf-8"))
	for asset in published_assets:
		asset["browser_download_url"] = (
			f"https://github.com/{CANONICAL_REPOSITORY}/releases/download/"
			f"v1.2.3/{asset['name']}"
		)
	assets_json.write_text(json.dumps(published_assets), encoding="utf-8")
	published_args = ["published" if value == "draft" else value for value in args]
	run(published_args)
	original_checksum = checksum.read_text(encoding="utf-8")
	checksum.write_text(f"{'0' * 64}  {ARCHIVE_NAME}\n", encoding="utf-8")
	expect_failure(published_args)
	checksum.write_text(original_checksum, encoding="utf-8")


def test_publisher(tmp: Path) -> None:
	fixture_root = tmp / "publisher"
	fixture_root.mkdir()
	local_archive, _, _, release_json, assets_json = write_artifact_fixture(fixture_root)
	public_fixture_root = tmp / "publisher-public"
	public_fixture_root.mkdir()
	(
		public_archive,
		public_appcast,
		public_checksum,
		public_release_json,
		public_assets_json,
	) = write_artifact_fixture(public_fixture_root)
	with zipfile.ZipFile(public_archive, "a") as public_bundle:
		public_bundle.comment = b"published bytes from an earlier nondeterministic build"
	public_appcast_tree = ET.parse(public_appcast)
	public_enclosure = public_appcast_tree.find("./channel/item/enclosure")
	assert public_enclosure is not None
	public_enclosure.set("length", str(public_archive.stat().st_size))
	public_appcast_tree.write(public_appcast, encoding="utf-8", xml_declaration=True)
	public_archive_digest = hashlib.sha256(public_archive.read_bytes()).hexdigest()
	public_checksum.write_text(
		f"{public_archive_digest}  {ARCHIVE_NAME}\n", encoding="utf-8"
	)
	public_release = json.loads(public_release_json.read_text(encoding="utf-8"))
	public_release["draft"] = False
	public_release["html_url"] = (
		f"https://github.com/{CANONICAL_REPOSITORY}/releases/tag/v1.2.3"
	)
	public_release_json.write_text(json.dumps(public_release), encoding="utf-8")
	public_paths = {
		path.name: path for path in (public_archive, public_appcast, public_checksum)
	}
	public_assets = json.loads(public_assets_json.read_text(encoding="utf-8"))
	for asset in public_assets:
		path = public_paths[asset["name"]]
		asset["size"] = path.stat().st_size
		asset["digest"] = f"sha256:{hashlib.sha256(path.read_bytes()).hexdigest()}"
		asset["browser_download_url"] = (
			f"https://github.com/{CANONICAL_REPOSITORY}/releases/download/"
			f"v1.2.3/{asset['name']}"
		)
	public_assets_json.write_text(json.dumps(public_assets), encoding="utf-8")
	assert local_archive.read_bytes() != public_archive.read_bytes()
	log_path = fixture_root / "gh.jsonl"
	fake_gh = fixture_root / "gh"
	fake_openssl = fixture_root / "openssl"
	write_executable(
		fake_openssl,
		"""#!/bin/sh
if [ "${FAKE_OPENSSL_FAIL:-0}" = "1" ]; then
    printf '%s\n' 'fixture signature mismatch' >&2
    exit 1
fi
exit 0
""",
	)
	write_executable(
		fake_gh,
		"""#!/usr/bin/env python3
import json
import os
import shutil
import sys
from pathlib import Path

args = sys.argv[1:]
with Path(os.environ["FAKE_GH_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(args) + "\\n")

if args[:2] == ["release", "view"]:
    state = os.environ.get("FAKE_GH_VIEW_STATE")
    calls = [
        json.loads(line)
        for line in Path(os.environ["FAKE_GH_LOG"]).read_text(encoding="utf-8").splitlines()
    ]
    release_created = any(call[:2] == ["release", "create"] for call in calls)
    if state in ("draft", "public") or release_created:
        print(json.dumps({
            "databaseId": 123,
            "isDraft": state != "public",
            "isPrerelease": False,
            "tagName": "v1.2.3",
        }))
        raise SystemExit(0)
    raise SystemExit(1)
if args[:2] in (["release", "create"], ["release", "upload"]):
    raise SystemExit(0)
if args[:2] == ["release", "download"]:
    destination = Path(args[args.index("--dir") + 1])
    name = args[args.index("--pattern") + 1]
    destination.mkdir(parents=True, exist_ok=True)
    input_dir = os.environ["FAKE_GH_INPUT_DIR"]
    if os.environ.get("FAKE_GH_VIEW_STATE") == "public":
        input_dir = os.environ.get("FAKE_GH_PUBLIC_INPUT_DIR", input_dir)
    shutil.copy2(Path(input_dir) / name, destination / name)
    if os.environ.get("FAKE_GH_CORRUPT_ASSET") == name:
        with (destination / name).open("ab") as handle:
            handle.write(b"corrupt")
    raise SystemExit(0)
if args and args[0] == "api":
    route = args[-1]
    if "--method" in args:
        raise SystemExit(0)
    if route.endswith("/git/ref/tags/v1.2.3"):
        print(json.dumps({"object": {"type": "tag", "sha": "b" * 40}}))
        raise SystemExit(0)
    if route.endswith("/git/tags/" + "b" * 40):
        commit = os.environ.get("FAKE_GH_TAG_COMMIT", "a" * 40)
        calls = [
            json.loads(line)
            for line in Path(os.environ["FAKE_GH_LOG"]).read_text(encoding="utf-8").splitlines()
        ]
        tag_object_reads = sum(
            1
            for call in calls
            if call and call[0] == "api" and call[-1].endswith("/git/tags/" + "b" * 40)
        )
        if tag_object_reads >= 2:
            commit = os.environ.get("FAKE_GH_TAG_COMMIT_AFTER_FIRST", commit)
        print(json.dumps({"tag": "v1.2.3", "object": {"type": "commit", "sha": commit}}))
        raise SystemExit(0)
    if "/releases/tags/" in route:
        raise SystemExit("draft releases must not be fetched by tag through REST")
    if route.endswith("/releases/123"):
        release_json = os.environ["FAKE_GH_RELEASE_JSON"]
        if os.environ.get("FAKE_GH_VIEW_STATE") == "public":
            release_json = os.environ.get("FAKE_GH_PUBLIC_RELEASE_JSON", release_json)
        release = json.loads(
            Path(release_json).read_text(encoding="utf-8")
        )
        if os.environ.get("FAKE_GH_VIEW_STATE") == "public":
            release["draft"] = False
            release["html_url"] = (
                "https://github.com/acg-box/rsnap/releases/tag/v1.2.3"
            )
        print(json.dumps(release))
        raise SystemExit(0)
    if "/releases/123/assets?" in route:
        assets_json = os.environ["FAKE_GH_ASSETS_JSON"]
        if os.environ.get("FAKE_GH_VIEW_STATE") == "public":
            assets_json = os.environ.get("FAKE_GH_PUBLIC_ASSETS_JSON", assets_json)
        assets = json.loads(
            Path(assets_json).read_text(encoding="utf-8")
        )
        if os.environ.get("FAKE_GH_VIEW_STATE") == "public":
            for asset in assets:
                asset["browser_download_url"] = (
                    "https://github.com/acg-box/rsnap/releases/download/"
                    f"v1.2.3/{asset['name']}"
                )
        print(json.dumps(assets))
        raise SystemExit(0)
raise SystemExit(f"unexpected fake gh invocation: {args}")
""",
	)

	env = os.environ.copy()
	env.update(
		{
			"FAKE_GH_ASSETS_JSON": str(assets_json),
			"FAKE_GH_INPUT_DIR": str(fixture_root),
			"FAKE_GH_LOG": str(log_path),
			"FAKE_GH_RELEASE_JSON": str(release_json),
			"GH_TOKEN": "fixture-token",
			"GITHUB_REPOSITORY": CANONICAL_REPOSITORY,
			"GITHUB_SHA": "a" * 40,
			"RSNAP_GH_BIN": str(fake_gh),
			"RSNAP_OPENSSL_BIN": str(fake_openssl),
			"RSNAP_RELEASE_COMMIT": "a" * 40,
			"RSNAP_RELEASE_INPUT_DIR": str(fixture_root),
			"RSNAP_RELEASE_TAG": "v1.2.3",
			"RSNAP_RELEASE_VERSION": "1.2.3",
			"RSNAP_SPARKLE_VERSION": "2.9.4",
			"RUNNER_TEMP": str(fixture_root),
		}
	)
	publisher = RELEASE_DIR / "publish-github-release.sh"
	run([publisher], env=env)
	calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	assert calls[-1][0] == "api"
	assert "PATCH" in calls[-1]
	assert any(call[:2] == ["release", "create"] for call in calls)

	log_path.unlink()
	existing_draft_env = dict(env)
	existing_draft_env["FAKE_GH_VIEW_STATE"] = "draft"
	run([publisher], env=existing_draft_env)
	existing_draft_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	]
	assert not any(call[:2] == ["release", "create"] for call in existing_draft_calls)
	assert any(call[:2] == ["release", "upload"] for call in existing_draft_calls)
	assert any(call[0] == "api" and "PATCH" in call for call in existing_draft_calls)

	log_path.unlink()
	invalid_env = dict(env)
	invalid_env["FAKE_GH_CORRUPT_ASSET"] = APPCAST_NAME
	expect_failure([publisher], env=invalid_env)
	failed_calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	assert not any(call[0] == "api" and "PATCH" in call for call in failed_calls)

	log_path.unlink()
	invalid_signature_env = dict(env)
	invalid_signature_env["FAKE_OPENSSL_FAIL"] = "1"
	expect_failure([publisher], env=invalid_signature_env)
	invalid_signature_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	]
	assert not any(
		call[0] == "api" and "PATCH" in call for call in invalid_signature_calls
	)

	log_path.unlink()
	moved_tag_env = dict(env)
	moved_tag_env["FAKE_GH_TAG_COMMIT"] = "c" * 40
	expect_failure([publisher], env=moved_tag_env)
	moved_tag_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	]
	assert not any(call[:2] == ["release", "create"] for call in moved_tag_calls)
	assert not any(call[0] == "api" and "PATCH" in call for call in moved_tag_calls)

	log_path.unlink()
	late_moved_tag_env = dict(env)
	late_moved_tag_env["FAKE_GH_TAG_COMMIT_AFTER_FIRST"] = "c" * 40
	expect_failure([publisher], env=late_moved_tag_env)
	late_moved_tag_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	]
	assert any(call[:2] == ["release", "create"] for call in late_moved_tag_calls)
	assert any(call[:2] == ["release", "upload"] for call in late_moved_tag_calls)
	assert not any(
		call[0] == "api" and "PATCH" in call for call in late_moved_tag_calls
	)

	log_path.unlink()
	public_env = dict(env)
	public_env["FAKE_GH_VIEW_STATE"] = "public"
	public_env["FAKE_GH_PUBLIC_ASSETS_JSON"] = str(public_assets_json)
	public_env["FAKE_GH_PUBLIC_INPUT_DIR"] = str(public_fixture_root)
	public_env["FAKE_GH_PUBLIC_RELEASE_JSON"] = str(public_release_json)
	run([publisher], env=public_env)
	public_calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	assert not any(call[:2] == ["release", "create"] for call in public_calls)
	assert not any(call[:2] == ["release", "upload"] for call in public_calls)
	assert not any(call[0] == "api" and "PATCH" in call for call in public_calls)
	assert sum(call[:2] == ["release", "download"] for call in public_calls) == 3


def test_package_orchestrator(tmp: Path) -> None:
	fixture_root = tmp / "package"
	tools = fixture_root / "tools"
	runner_temp = fixture_root / "runner-temp"
	output = fixture_root / "output"
	tools.mkdir(parents=True)
	runner_temp.mkdir()
	log_path = fixture_root / "tools.jsonl"

	def tool(name: str, body: str) -> Path:
		path = tools / name
		write_executable(path, body)
		return path

	fake_uname = tool("uname", "#!/bin/sh\nprintf 'arm64\\n'\n")
	fake_security = tool(
		"security",
		"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path
args = sys.argv[1:]
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["security", *args]) + "\\n")
if args and args[0] == "create-keychain":
    Path(args[-1]).touch()
elif args and args[0] == "find-identity":
    print('  1) AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA "Developer ID Application: Rsnap Release (TEAM123)"')
""",
	)
	fake_build = tool(
		"build",
		f"""#!/usr/bin/env python3
import os
import plistlib
from pathlib import Path
secret_names = (
    "APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
    "APPLE_NOTARY_ISSUER_ID",
    "APPLE_NOTARY_KEY_ID",
    "APPLE_NOTARY_KEY_P8",
    "RSNAP_SPARKLE_PRIVATE_ED_KEY",
)
leaked = [name for name in secret_names if name in os.environ]
if leaked:
    raise SystemExit(f"release credentials leaked to build: {{leaked}}")
app = Path(os.environ["RSNAP_NATIVE_HOST_STAGE_DIR"]) / "Rsnap.app"
work_root = app.parents[1]
if (work_root / "developer-id.p12").exists() or list(work_root.glob("AuthKey_*.p8")):
    raise SystemExit("release credentials were materialized before the build completed")
(app / "Contents/MacOS").mkdir(parents=True)
(app / "Contents/MacOS/RsnapNativeHost").write_bytes(b"mach-o")
(app / "Contents/Info.plist").write_bytes(plistlib.dumps({{"SUPublicEDKey": "{PUBLIC_KEY}"}}))
""",
	)
	fake_key_verifier = tool("key-verifier", "#!/bin/sh\ncat >/dev/null\n")
	fake_sign = tool(
		"sign",
		"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["sign", *sys.argv[1:]]) + "\\n")
""",
	)
	fake_validator = tool("validator", "#!/bin/sh\nexit 0\n")
	fake_codesign = tool(
		"codesign",
		"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["codesign", *sys.argv[1:]]) + "\\n")
""",
	)
	fake_spctl = tool(
		"spctl",
		"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["spctl", *sys.argv[1:]]) + "\\n")
""",
	)
	fake_ditto = tool(
		"ditto",
		"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["ditto", *args]) + "\\n")
Path(args[-1]).write_bytes(b"zip-bytes")
""",
	)
	fake_xcrun = tool(
		"xcrun",
		"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["xcrun", *args]) + "\\n")
if args and args[0] == "notarytool":
    submission_id = "12345678-1234-1234-1234-123456789abc"
    if args[1] == "submit":
        print(json.dumps({"id": submission_id, "message": "Successfully uploaded file"}))
    elif args[1] == "wait":
        if os.environ.get("FAKE_NOTARY_WAIT_FAIL") == "1":
            raise SystemExit(1)
        print(json.dumps({
            "status": os.environ.get("FAKE_NOTARY_STATUS", "Accepted"),
            "id": submission_id,
        }))
""",
	)
	fake_appcast = tool(
		"appcast",
		"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["appcast", *args]) + "\\n")
Path(args[args.index("--appcast") + 1]).write_text("<rss/>", encoding="utf-8")
""",
	)

	env = os.environ.copy()
	for key in ("SPARKLE_PRIVATE_ED_KEY",):
		env.pop(key, None)
	env.update(
		{
			"APPLE_DEVELOPER_ID_APPLICATION_IDENTITY": (
				"Developer ID Application: Rsnap Release (TEAM123)"
			),
			"APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64": base64.b64encode(b"cert").decode(),
			"APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD": "fixture-password",
			"APPLE_NOTARY_ISSUER_ID": "fixture-issuer",
			"APPLE_NOTARY_KEY_ID": "FIXTURE123",
			"APPLE_NOTARY_KEY_P8": "fixture-p8",
			"RELEASE_TOOL_LOG": str(log_path),
			"RSNAP_APPCAST_BIN": str(fake_appcast),
			"RSNAP_ARTIFACT_VALIDATOR_BIN": str(fake_validator),
			"RSNAP_BUILD_AND_RUN_BIN": str(fake_build),
			"RSNAP_CODESIGN_BIN": str(fake_codesign),
			"RSNAP_DITTO_BIN": str(fake_ditto),
			"RSNAP_PYTHON_BIN": sys.executable,
			"RSNAP_RELEASE_OUTPUT_DIR": str(output),
			"RSNAP_RELEASE_TAG": "v1.2.3",
			"RSNAP_RELEASE_VERSION": "1.2.3",
			"RSNAP_SECURITY_BIN": str(fake_security),
			"RSNAP_SIGN_APP_BIN": str(fake_sign),
			"RSNAP_SPARKLE_PRIVATE_ED_KEY": "fixture-private-key",
			"RSNAP_SPARKLE_VERSION": "2.9.4",
			"RSNAP_SPCTL_BIN": str(fake_spctl),
			"RSNAP_UNAME_BIN": str(fake_uname),
			"RSNAP_VERIFY_SPARKLE_KEY_BIN": str(fake_key_verifier),
			"RSNAP_XCRUN_BIN": str(fake_xcrun),
			"RUNNER_ARCH": "ARM64",
			"RUNNER_TEMP": str(runner_temp),
		}
	)
	package_script = RELEASE_DIR / "package-macos.sh"
	run([package_script], env=env)
	assert (output / ARCHIVE_NAME).is_file()
	assert (output / APPCAST_NAME).is_file()
	assert (output / CHECKSUM_NAME).is_file()
	calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	tool_names = [call[0] for call in calls]
	assert tool_names.index("sign") < tool_names.index("xcrun")
	notary_index = next(
		index for index, call in enumerate(calls) if call[:3] == ["xcrun", "notarytool", "submit"]
	)
	staple_index = next(
		index for index, call in enumerate(calls) if call[:3] == ["xcrun", "stapler", "staple"]
	)
	spctl_index = tool_names.index("spctl")
	final_zip_index = max(index for index, call in enumerate(calls) if call[0] == "ditto")
	appcast_index = tool_names.index("appcast")
	assert notary_index < staple_index < spctl_index < final_zip_index < appcast_index

	invalid_output = fixture_root / "invalid-output"
	invalid_env = dict(env)
	invalid_env["FAKE_NOTARY_STATUS"] = "Invalid"
	invalid_env["RSNAP_RELEASE_OUTPUT_DIR"] = str(invalid_output)
	expect_failure([package_script], env=invalid_env)
	assert not (invalid_output / ARCHIVE_NAME).exists()
	assert not (invalid_output / APPCAST_NAME).exists()

	timeout_output = fixture_root / "timeout-output"
	timeout_env = dict(env)
	timeout_env["FAKE_NOTARY_WAIT_FAIL"] = "1"
	timeout_env["RSNAP_RELEASE_OUTPUT_DIR"] = str(timeout_output)
	log_line_count = len(log_path.read_text(encoding="utf-8").splitlines())
	timeout_result = run([package_script], env=timeout_env, check=False)
	assert timeout_result.returncode != 0
	assert "12345678-1234-1234-1234-123456789abc" in timeout_result.stderr
	timeout_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	][log_line_count:]
	assert not any(call[:3] == ["xcrun", "stapler", "staple"] for call in timeout_calls)
	assert not (timeout_output / ARCHIVE_NAME).exists()

	missing_credential_env = dict(env)
	missing_credential_env.pop("APPLE_NOTARY_ISSUER_ID")
	log_before_missing_credential = log_path.read_text(encoding="utf-8")
	expect_failure([package_script], env=missing_credential_env)
	assert log_path.read_text(encoding="utf-8") == log_before_missing_credential


def test_static_contracts() -> None:
	assert not (ROOT / ".github/workflows/one-time-sparkle-secret-relay.yml").exists()
	for script in (
		ROOT / "scripts/build_and_run.sh",
		RELEASE_DIR / "package-macos.sh",
		RELEASE_DIR / "publish-github-release.sh",
		RELEASE_DIR / "sign-macos-app.sh",
		RELEASE_DIR / "sparkle-appcast.sh",
		ROOT / "scripts/smoke/sparkle-update-local.sh",
	):
		run(["bash", "-n", script])
	for script in (
		RELEASE_DIR / "self-check.py",
		RELEASE_DIR / "validate-release-artifacts.py",
		RELEASE_DIR / "validate-release-source.py",
	):
		compile(script.read_text(encoding="utf-8"), str(script), "exec")

	release_workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
	language_workflow = (ROOT / ".github/workflows/language.yml").read_text(encoding="utf-8")
	release_spec = (ROOT / "docs/spec/release-distribution.md").read_text(encoding="utf-8")
	release_runbook = (ROOT / "docs/runbook/validate-release.md").read_text(encoding="utf-8")
	infisical_config = json.loads((ROOT / ".infisical.json").read_text(encoding="utf-8"))
	publisher_script = (RELEASE_DIR / "publish-github-release.sh").read_text(
		encoding="utf-8"
	)
	sparkle_smoke = (ROOT / "scripts/smoke/sparkle-update-local.sh").read_text(
		encoding="utf-8"
	)
	assert "workflow_dispatch" not in release_workflow
	assert "Release Preparation" not in release_workflow
	assert "Release Prep" not in release_workflow
	assert "Release Dry Run" not in release_workflow
	assert "permissions:\n  contents: read" in release_workflow
	assert release_workflow.count("contents: write") == 1
	assert release_workflow.count("name: release") == 2
	assert "runs-on: macos-26" in release_workflow
	assert "needs: validate-release" in release_workflow
	assert "needs: [validate-release, build-macos]" in release_workflow
	assert (
		"RSNAP_SPARKLE_PRIVATE_ED_KEY: ${{ secrets.RSNAP_SPARKLE_PRIVATE_ED_KEY }}"
		in release_workflow
	)
	assert "${{ secrets.SPARKLE_PRIVATE_ED_KEY }}" not in release_workflow
	assert re.search(r"^\s+SPARKLE_PRIVATE_ED_KEY:", release_workflow, re.MULTILINE) is None
	for release_document in (release_spec, release_runbook):
		normalized_release_document = " ".join(release_document.split())
		assert "`acg-box` organization" in normalized_release_document
		assert "visibility `all`" in normalized_release_document
		assert "visibility `selected`" not in normalized_release_document
		assert "name `v*` and type `tag`" in normalized_release_document
		assert "configured in that environment" not in release_document
	assert "another application must use a separately named private key" in " ".join(
		release_spec.split()
	)
	assert infisical_config == {
		"workspaceId": "f55a1068-0ae7-4dee-a0c0-62bfe71016fc",
		"defaultEnvironment": "prod",
		"gitBranchToEnvironmentMapping": None,
		"domain": "http://127.0.0.1:51890",
	}
	for release_document in (release_spec, release_runbook):
		normalized_release_document = " ".join(release_document.split())
		assert "`/release/apple`" in normalized_release_document
		assert "`/release/sparkle`" in normalized_release_document
		assert "`rsnap-release-provisioner`" in normalized_release_document
	assert "--verify-appcast-signature" in publisher_script
	assert "releases/tags/" not in publisher_script
	assert 'rm -rf "$WORK_ROOT"' not in sparkle_smoke
	assert 'mktemp -d "$WORK_PARENT/rsnap-sparkle-update-smoke.XXXXXX"' in sparkle_smoke
	for match in re.finditer(r"^\s*uses:\s*[^\s]+@([^\s]+)", release_workflow, re.MULTILINE):
		assert re.fullmatch(r"[0-9a-f]{40}", match.group(1)), match.group(0)
	for required in (
		"rust-check:",
		"swift-check:",
		"toml-check:",
		"runs-on: macos-26",
		"pull_request:",
		"merge_group:",
	):
		assert required in language_workflow

	tracked_paths = run(
		["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
		cwd=ROOT,
	).stdout.split("\0")
	tracked_files = [
		ROOT / relative_path
		for relative_path in tracked_paths
		if relative_path and (ROOT / relative_path).is_file()
	]
	tracked_text = "\n".join(
		path.read_text(encoding="utf-8", errors="ignore")
		for path in tracked_files
		if path.stat().st_size < 2_000_000
	)
	assert ("acg" + "xv/rsnap") not in tracked_text


def main() -> int:
	with tempfile.TemporaryDirectory(prefix="rsnap-release-self-check-") as temp_dir:
		tmp = Path(temp_dir)
		test_source_validator(tmp)
		test_signer(tmp)
		test_appcast(tmp)
		test_sparkle_key_verifier(tmp)
		test_artifact_validator(tmp)
		test_publisher(tmp)
		test_package_orchestrator(tmp)
	test_static_contracts()
	print("release self-check passed")
	return 0


if __name__ == "__main__":
	raise SystemExit(main())
