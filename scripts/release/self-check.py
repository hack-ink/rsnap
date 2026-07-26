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
import stat
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
UNSIGNED_ARCHIVE_NAME = "rsnap-unsigned-aarch64-apple-darwin.zip"
UNSIGNED_MANIFEST_NAME = "rsnap-unsigned-aarch64-apple-darwin.json"
APPCAST_NAME = "appcast.xml"
CHECKSUM_NAME = f"{ARCHIVE_NAME}.sha256"
PUBLIC_KEY = "X2EaTv6mCzkYxz75Hh+ldMkKlpzNlHRg5l7Kn9ke8Ow="
SPARKLE_NAMESPACE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
SOURCE_COMMIT = "a" * 40
EXPECTED_APP_SYMLINKS = {
	"Contents/Frameworks/Sparkle.framework/Autoupdate": "Versions/Current/Autoupdate",
	"Contents/Frameworks/Sparkle.framework/Headers": "Versions/Current/Headers",
	"Contents/Frameworks/Sparkle.framework/Modules": "Versions/Current/Modules",
	"Contents/Frameworks/Sparkle.framework/PrivateHeaders": (
		"Versions/Current/PrivateHeaders"
	),
	"Contents/Frameworks/Sparkle.framework/Resources": "Versions/Current/Resources",
	"Contents/Frameworks/Sparkle.framework/Sparkle": "Versions/Current/Sparkle",
	"Contents/Frameworks/Sparkle.framework/Updater.app": "Versions/Current/Updater.app",
	"Contents/Frameworks/Sparkle.framework/Versions/Current": "B",
	"Contents/Frameworks/Sparkle.framework/XPCServices": "Versions/Current/XPCServices",
}
EXPECTED_APP_EXECUTABLES = {
	"Contents/MacOS/RsnapNativeHost",
	"Contents/Frameworks/Sparkle.framework/Versions/B/Autoupdate",
	"Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle",
	"Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app/Contents/MacOS/Updater",
	"Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Downloader.xpc/Contents/MacOS/Downloader",
	"Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Installer.xpc/Contents/MacOS/Installer",
}


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

	# A tag that was the main tip when it was created is no longer a valid source after main moves.
	git(repo, "commit", "--allow-empty", "-m", "main advanced after tag")
	advanced_main_commit = git(repo, "rev-parse", "HEAD")
	git(repo, "update-ref", "refs/remotes/origin/main", advanced_main_commit)
	git(repo, "switch", "--detach", tag_commit)
	expect_failure(base_args)

	# A commit on a different line of history is also not an accepted main tip.
	tree = git(repo, "rev-parse", f"{base_commit}^{{tree}}")
	divergent_main_commit = run(
		["git", "commit-tree", tree, "-p", base_commit],
		cwd=repo,
		input_text="divergent main\n",
	).stdout.strip()
	git(repo, "update-ref", "refs/remotes/origin/main", divergent_main_commit)
	expect_failure(base_args)
	git(repo, "update-ref", "refs/remotes/origin/main", tag_commit)

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
	log_path = fixture_root / "sparkle-signer.jsonl"
	sparkle_signer = fixture_root / "sparkle-signer"
	write_executable(
		sparkle_signer,
		"""#!/usr/bin/env python3
import base64
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
leaked = [
    name for name in ("RSNAP_SPARKLE_PRIVATE_ED_KEY", "RSNAP_SPARKLE_PUBLIC_ED_KEY")
    if name in os.environ
]
if leaked:
    raise SystemExit(f"Sparkle key leaked through signer environment: {leaked}")
with Path(os.environ["SIGN_UPDATE_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(args) + "\\n")
if sys.stdin.read().strip() != "fixture-private-key":
    raise SystemExit("signer did not receive the fixture private key on standard input")
print(base64.b64encode(bytes(64)).decode("ascii"))
""",
	)
	env = os.environ.copy()
	env.update(
		{
			"RSNAP_SPARKLE_PRIVATE_ED_KEY": "fixture-private-key",
			"RSNAP_SPARKLE_PUBLIC_ED_KEY": PUBLIC_KEY,
			"RSNAP_SPARKLE_SIGNER_BIN": str(sparkle_signer),
			"SIGN_UPDATE_LOG": str(log_path),
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
	assert calls == [[str(archive), PUBLIC_KEY]]

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

	missing_public_env = dict(env)
	missing_public_env.pop("RSNAP_SPARKLE_PUBLIC_ED_KEY")
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
		env=missing_public_env,
	)


def test_sparkle_key_verifier(tmp: Path) -> None:
	if sys.platform != "darwin":
		return
	swift = shutil.which("swift")
	if swift is None:
		raise AssertionError("swift is required for the macOS Sparkle key self-check")
	openssl = shutil.which("openssl")
	if openssl is None:
		raise AssertionError("OpenSSL is required for the macOS Sparkle signer self-check")
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

	archive = tmp / "sparkle-signer-fixture.zip"
	archive.write_bytes(b"fixture update archive")
	signer = RELEASE_DIR / "sign-sparkle-update.swift"
	signature_text = run(
		[signer, archive, public_key], input_text=f"{private_key}\n"
	).stdout.strip()
	signature = base64.b64decode(signature_text, validate=True)
	assert len(signature) == 64
	public_key_der = bytes.fromhex("302a300506032b6570032100") + base64.b64decode(
		public_key, validate=True
	)
	public_key_path = tmp / "sparkle-public.der"
	signature_path = tmp / "sparkle-signature.bin"
	public_key_path.write_bytes(public_key_der)
	signature_path.write_bytes(signature)
	verify_args = [
		openssl,
		"pkeyutl",
		"-verify",
		"-pubin",
		"-inkey",
		public_key_path,
		"-keyform",
		"DER",
		"-rawin",
		"-in",
		archive,
		"-sigfile",
		signature_path,
	]
	run(verify_args)
	archive.write_bytes(b"tampered update archive")
	expect_failure(verify_args)
	expect_failure([signer, archive, wrong_public_key], input_text=f"{private_key}\n")


def zip_entry(name: str, data: bytes, mode: int) -> tuple[zipfile.ZipInfo, bytes]:
	info = zipfile.ZipInfo(name)
	info.create_system = 3
	info.external_attr = mode << 16
	return info, data


def write_unsigned_handoff_fixture(
	root: Path,
	*,
	extra_entries: list[tuple[str, bytes, int]] | None = None,
	manifest_overrides: dict[str, object] | None = None,
) -> tuple[Path, Path, str]:
	root.mkdir(parents=True)
	archive = root / UNSIGNED_ARCHIVE_NAME
	manifest = root / UNSIGNED_MANIFEST_NAME
	main_plist = plistlib.dumps(
		{
			"CFBundleName": "Rsnap",
			"CFBundleDisplayName": "Rsnap",
			"CFBundleIdentifier": "ink.hack.rsnap",
			"CFBundleShortVersionString": "1.2.3",
			"CFBundleVersion": "1.2.3",
			"LSMinimumSystemVersion": "14.0",
			"SUFeedURL": (
				f"https://github.com/{CANONICAL_REPOSITORY}/releases/latest/download/"
				f"{APPCAST_NAME}"
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
	regular_entries: list[tuple[str, bytes, int]] = [
		("Rsnap.app/Contents/Info.plist", main_plist, stat.S_IFREG | 0o644),
		(
			"Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/B/Resources/"
			"Info.plist",
			framework_plist,
			stat.S_IFREG | 0o644,
		),
	]
	for relative_path in (
		"Updater.app/Contents/Info.plist",
		"XPCServices/Installer.xpc/Contents/Info.plist",
		"XPCServices/Downloader.xpc/Contents/Info.plist",
	):
		regular_entries.append(
			(
				"Rsnap.app/Contents/Frameworks/Sparkle.framework/Versions/B/"
				+ relative_path,
				b"fixture",
				stat.S_IFREG | 0o644,
			)
		)
	for relative_path in sorted(EXPECTED_APP_EXECUTABLES):
		regular_entries.append(
			(f"Rsnap.app/{relative_path}", b"mach-o", stat.S_IFREG | 0o755)
		)
	for relative_path, target in sorted(EXPECTED_APP_SYMLINKS.items()):
		regular_entries.append(
			(f"Rsnap.app/{relative_path}", target.encode(), stat.S_IFLNK | 0o755)
		)
	if extra_entries:
		regular_entries.extend(extra_entries)
	with zipfile.ZipFile(archive, "w") as bundle:
		for name, data, mode in regular_entries:
			info, contents = zip_entry(name, data, mode)
			bundle.writestr(info, contents)

	digest = hashlib.sha256(archive.read_bytes()).hexdigest()
	manifest_value: dict[str, object] = {
		"schema": "rsnap-unsigned-macos-handoff/1",
		"repository": CANONICAL_REPOSITORY,
		"source_commit": SOURCE_COMMIT,
		"tag": "v1.2.3",
		"version": "1.2.3",
		"sparkle_version": "2.9.4",
		"archive": {"name": UNSIGNED_ARCHIVE_NAME, "sha256": digest},
	}
	if manifest_overrides:
		manifest_value.update(manifest_overrides)
	manifest.write_text(
		json.dumps(manifest_value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
	)
	return archive, manifest, digest


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

	unsigned_archive, unsigned_manifest, unsigned_digest = write_unsigned_handoff_fixture(
		tmp / "unsigned-artifact"
	)
	unsigned_args = [
		validator,
		"--source-commit",
		SOURCE_COMMIT,
		"--unsigned-archive",
		unsigned_archive,
		"--unsigned-archive-sha256",
		unsigned_digest,
		"--unsigned-manifest",
		unsigned_manifest,
		"--version",
		"1.2.3",
		"--sparkle-version",
		"2.9.4",
		"--tag",
		"v1.2.3",
		"--repository",
		CANONICAL_REPOSITORY,
	]
	run(unsigned_args)
	wrong_digest_args = list(unsigned_args)
	wrong_digest_args[wrong_digest_args.index("--unsigned-archive-sha256") + 1] = (
		"0" * 64
	)
	expect_failure(wrong_digest_args)
	wrong_commit_args = list(unsigned_args)
	wrong_commit_args[wrong_commit_args.index("--source-commit") + 1] = "b" * 40
	expect_failure(wrong_commit_args)

	unsafe_fixtures = (
		(
			"traversal",
			[("Rsnap.app/../escape", b"escape", stat.S_IFREG | 0o644)],
		),
		(
			"unexpected-symlink",
			[("Rsnap.app/Contents/escape", b"/tmp", stat.S_IFLNK | 0o755)],
		),
		(
			"unexpected-executable",
			[("Rsnap.app/Contents/Resources/tool", b"tool", stat.S_IFREG | 0o755)],
		),
		(
			"duplicate-normalized-path",
			[("Rsnap.app/Contents/Info.plist/", b"", stat.S_IFDIR | 0o755)],
		),
		(
			"special-file",
			[("Rsnap.app/Contents/fifo", b"", stat.S_IFIFO | 0o644)],
		),
	)
	for fixture_name, extra_entries in unsafe_fixtures:
		bad_archive, bad_manifest, bad_digest = write_unsigned_handoff_fixture(
			tmp / f"unsigned-{fixture_name}", extra_entries=extra_entries
		)
		bad_args = [
			str(value)
			for value in unsigned_args
		]
		bad_args[bad_args.index(str(unsigned_archive))] = str(bad_archive)
		bad_args[bad_args.index(str(unsigned_manifest))] = str(bad_manifest)
		bad_args[bad_args.index(unsigned_digest)] = bad_digest
		expect_failure(bad_args)


def test_release_order_validator(tmp: Path) -> None:
	validator = RELEASE_DIR / "validate-release-order.py"
	inventory_path = tmp / "release-inventory.json"

	def release(
		tag: str,
		*,
		draft: bool = False,
		prerelease: bool = False,
		latest: bool = False,
	) -> dict[str, object]:
		return {
			"tagName": tag,
			"isDraft": draft,
			"isPrerelease": prerelease,
			"isLatest": latest,
		}

	def validate(inventory: list[dict[str, object]], *, expect: str | None) -> None:
		inventory_path.write_text(json.dumps(inventory), encoding="utf-8")
		args = [
			validator,
			"--releases-json",
			inventory_path,
			"--tag",
			"v1.2.3",
			"--version",
			"1.2.3",
			"--inventory-limit",
			"1000",
		]
		if expect is None:
			expect_failure(args)
		else:
			assert run(args).stdout.strip() == expect

	validate([], expect="absent")
	validate([release("v1.2.2", latest=True)], expect="absent")
	validate(
		[release("v1.2.2", latest=True), release("v1.2.3", draft=True)],
		expect="draft",
	)
	validate(
		[release("v1.2.2"), release("v1.2.3", latest=True)],
		expect="published",
	)
	# A retry of immutable public bytes remains read-only even after a newer stable release.
	validate(
		[release("v1.2.3"), release("v1.2.4", latest=True)],
		expect="published",
	)
	validate([release("v1.2.4", latest=True)], expect=None)
	validate([release("v1.2.3", prerelease=True)], expect=None)
	validate(
		[release("v1.2.1", latest=True), release("v1.2.2")],
		expect=None,
	)
	validate(
		[release(f"draft-{index}", draft=True) for index in range(1000)],
		expect=None,
	)


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

if args[:2] == ["release", "list"]:
    calls = [
        json.loads(line)
        for line in Path(os.environ["FAKE_GH_LOG"]).read_text(encoding="utf-8").splitlines()
    ]
    list_call_count = sum(call[:2] == ["release", "list"] for call in calls)
    release_created = any(call[:2] == ["release", "create"] for call in calls)
    state = os.environ.get("FAKE_GH_VIEW_STATE")
    mode = os.environ.get("FAKE_GH_INVENTORY_MODE", "default")

    def release(tag, *, draft=False, prerelease=False, latest=False):
        return {
            "tagName": tag,
            "isDraft": draft,
            "isPrerelease": prerelease,
            "isLatest": latest,
        }

    if mode == "saturated":
        inventory = [release(f"draft-{index}", draft=True) for index in range(1000)]
    elif mode == "stale-latest":
        inventory = [release("v1.2.1", latest=True), release("v1.2.2")]
    elif mode == "downgrade":
        inventory = [release("v1.2.4", latest=True)]
    elif mode == "prerelease":
        inventory = [release("v1.2.3", prerelease=True)]
    elif mode == "older-public-retry":
        inventory = [release("v1.2.3"), release("v1.2.4", latest=True)]
    elif mode == "concurrent-newer" and list_call_count >= 2:
        inventory = [
            release("v1.2.2"),
            release("v1.2.3", draft=True),
            release("v1.2.4", latest=True),
        ]
    else:
        inventory = []
        if mode != "no-releases":
            inventory.append(release("v1.2.2", latest=state != "public"))
        if state == "public":
            inventory.append(release("v1.2.3", latest=True))
        elif state == "draft" or release_created:
            inventory.append(release("v1.2.3", draft=True))
    print(json.dumps(inventory))
    raise SystemExit(0)
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
	assert "make_latest=legacy" in calls[-1]
	assert "make_latest=true" not in calls[-1]
	assert calls[-2][:2] == ["release", "list"]
	assert sum(call[:2] == ["release", "list"] for call in calls) == 2
	assert any(call[:2] == ["release", "create"] for call in calls)

	log_path.unlink()
	no_release_env = dict(env)
	no_release_env["FAKE_GH_INVENTORY_MODE"] = "no-releases"
	run([publisher], env=no_release_env)
	no_release_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	]
	assert any(call[:2] == ["release", "create"] for call in no_release_calls)
	assert "make_latest=legacy" in no_release_calls[-1]

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

	log_path.unlink()
	older_public_env = dict(public_env)
	older_public_env["FAKE_GH_INVENTORY_MODE"] = "older-public-retry"
	run([publisher], env=older_public_env)
	older_public_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	]
	assert not any(call[:2] == ["release", "create"] for call in older_public_calls)
	assert not any(call[:2] == ["release", "upload"] for call in older_public_calls)
	assert not any(call[0] == "api" and "PATCH" in call for call in older_public_calls)

	for unsafe_mode in ("downgrade", "prerelease", "stale-latest", "saturated"):
		log_path.unlink()
		unsafe_env = dict(env)
		unsafe_env["FAKE_GH_INVENTORY_MODE"] = unsafe_mode
		expect_failure([publisher], env=unsafe_env)
		unsafe_calls = [
			json.loads(line)
			for line in log_path.read_text(encoding="utf-8").splitlines()
		]
		assert sum(call[:2] == ["release", "list"] for call in unsafe_calls) == 1
		assert not any(call[:2] == ["release", "create"] for call in unsafe_calls)
		assert not any(call[:2] == ["release", "upload"] for call in unsafe_calls)
		assert not any(call[0] == "api" and "PATCH" in call for call in unsafe_calls)

	log_path.unlink()
	concurrent_env = dict(env)
	concurrent_env["FAKE_GH_INVENTORY_MODE"] = "concurrent-newer"
	expect_failure([publisher], env=concurrent_env)
	concurrent_calls = [
		json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()
	]
	assert sum(call[:2] == ["release", "list"] for call in concurrent_calls) == 2
	assert any(call[:2] == ["release", "create"] for call in concurrent_calls)
	assert any(call[:2] == ["release", "upload"] for call in concurrent_calls)
	assert not any(call[0] == "api" and "PATCH" in call for call in concurrent_calls)


def test_unsigned_build_orchestrator(tmp: Path) -> None:
	fixture_root = tmp / "unsigned-build"
	tools = fixture_root / "tools"
	runner_temp = fixture_root / "runner-temp"
	output = fixture_root / "output"
	tools.mkdir(parents=True)
	runner_temp.mkdir()
	log_path = fixture_root / "tools.jsonl"
	github_output = fixture_root / "github-output"

	def tool(name: str, body: str) -> Path:
		path = tools / name
		write_executable(path, body)
		return path

	fake_uname = tool("uname", "#!/bin/sh\nprintf 'arm64\\n'\n")
	fake_git = tool(
		"git",
		f"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["git", *args]) + "\\n")
if "rev-parse" in args:
    print(os.environ.get("FAKE_GIT_COMMIT", "{SOURCE_COMMIT}"))
elif "status" in args:
    print(os.environ.get("FAKE_GIT_STATUS", ""), end="")
else:
    raise SystemExit(f"unexpected fake git invocation: {{args}}")
""",
	)
	fake_build = tool(
		"build",
		"""#!/usr/bin/env python3
import json
import os
import plistlib
import sys
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
command_file_names = (
    "BASH_ENV",
    "ENV",
    "GITHUB_ENV",
    "GITHUB_OUTPUT",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
)
leaked = [name for name in (*secret_names, *command_file_names) if name in os.environ]
if leaked:
    raise SystemExit(f"unsigned build received forbidden environment values: {leaked}")
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["build", *sys.argv[1:]]) + "\\n")
app = Path(os.environ["RSNAP_NATIVE_HOST_STAGE_DIR"]) / "Rsnap.app"
(app / "Contents/MacOS").mkdir(parents=True)
(app / "Contents/MacOS/RsnapNativeHost").write_bytes(b"mach-o")
(app / "Contents/Info.plist").write_bytes(plistlib.dumps({"fixture": True}))
""",
	)
	fake_ditto = tool(
		"ditto",
		"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["ditto", *args]) + "\\n")
Path(args[-1]).write_bytes(b"unsigned-zip-bytes")
""",
	)
	fake_validator = tool(
		"validator",
		"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

forbidden = (
    "APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
    "APPLE_NOTARY_ISSUER_ID",
    "APPLE_NOTARY_KEY_ID",
    "APPLE_NOTARY_KEY_P8",
    "RSNAP_SPARKLE_PRIVATE_ED_KEY",
    "BASH_ENV",
    "ENV",
    "GITHUB_ENV",
    "GITHUB_OUTPUT",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
)
leaked = [name for name in forbidden if name in os.environ]
if leaked:
    raise SystemExit(f"unsigned validator received forbidden values: {leaked}")
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["validator", *sys.argv[1:]]) + "\\n")
""",
	)

	env = os.environ.copy()
	for key in (
		"APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
		"APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
		"APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
		"APPLE_NOTARY_ISSUER_ID",
		"APPLE_NOTARY_KEY_ID",
		"APPLE_NOTARY_KEY_P8",
		"RSNAP_SPARKLE_PRIVATE_ED_KEY",
	):
		env.pop(key, None)
	env.update(
		{
			"BASH_ENV": "/dev/null",
			"ENV": "/dev/null",
			"GITHUB_ENV": str(fixture_root / "github-env"),
			"GITHUB_OUTPUT": str(github_output),
			"GITHUB_PATH": str(fixture_root / "github-path"),
			"GITHUB_STEP_SUMMARY": str(fixture_root / "github-summary"),
			"RELEASE_TOOL_LOG": str(log_path),
			"RSNAP_ARTIFACT_VALIDATOR_BIN": str(fake_validator),
			"RSNAP_BUILD_AND_RUN_BIN": str(fake_build),
			"RSNAP_DITTO_BIN": str(fake_ditto),
			"RSNAP_GIT_BIN": str(fake_git),
			"RSNAP_PYTHON_BIN": sys.executable,
			"RSNAP_RELEASE_COMMIT": SOURCE_COMMIT,
			"RSNAP_RELEASE_TAG": "v1.2.3",
			"RSNAP_RELEASE_VERSION": "1.2.3",
			"RSNAP_SPARKLE_VERSION": "2.9.4",
			"RSNAP_UNAME_BIN": str(fake_uname),
			"RSNAP_UNSIGNED_OUTPUT_DIR": str(output),
			"RUNNER_ARCH": "ARM64",
			"RUNNER_TEMP": str(runner_temp),
		}
	)
	build_script = RELEASE_DIR / "build-unsigned-macos.sh"
	run([build_script], env=env)
	archive = output / UNSIGNED_ARCHIVE_NAME
	manifest = output / UNSIGNED_MANIFEST_NAME
	assert archive.read_bytes() == b"unsigned-zip-bytes"
	digest = hashlib.sha256(archive.read_bytes()).hexdigest()
	assert github_output.read_text(encoding="utf-8") == f"archive_sha256={digest}\n"
	assert json.loads(manifest.read_text(encoding="utf-8")) == {
		"schema": "rsnap-unsigned-macos-handoff/1",
		"repository": CANONICAL_REPOSITORY,
		"source_commit": SOURCE_COMMIT,
		"tag": "v1.2.3",
		"version": "1.2.3",
		"sparkle_version": "2.9.4",
		"archive": {"name": UNSIGNED_ARCHIVE_NAME, "sha256": digest},
	}
	calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	validator_calls = [call for call in calls if call[0] == "validator"]
	assert len(validator_calls) == 3
	assert "--app" in validator_calls[0]
	assert "--unsigned-archive" in validator_calls[1]
	assert "--source-commit" in validator_calls[2]
	assert validator_calls[2][validator_calls[2].index("--source-commit") + 1] == SOURCE_COMMIT
	assert "--unsigned-manifest" in validator_calls[2]

	secret_env = dict(env)
	secret_env["APPLE_NOTARY_KEY_P8"] = "must-not-enter-build-job"
	log_before_secret = log_path.read_text(encoding="utf-8")
	expect_failure([build_script], env=secret_env)
	assert log_path.read_text(encoding="utf-8") == log_before_secret

	wrong_commit_env = dict(env)
	wrong_commit_env["FAKE_GIT_COMMIT"] = "b" * 40
	expect_failure([build_script], env=wrong_commit_env)
	assert not archive.exists()
	assert not manifest.exists()


def test_package_orchestrator(tmp: Path) -> None:
	fixture_root = tmp / "package"
	tools = fixture_root / "tools"
	runner_temp = fixture_root / "runner-temp"
	output = fixture_root / "output"
	tools.mkdir(parents=True)
	runner_temp.mkdir()
	log_path = fixture_root / "tools.jsonl"
	handoff_archive, handoff_manifest, handoff_digest = write_unsigned_handoff_fixture(
		fixture_root / "handoff"
	)

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
	fake_key_verifier = tool(
		"key-verifier",
		"""#!/usr/bin/env python3
import os
import sys

forbidden = (
    "APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
    "APPLE_NOTARY_ISSUER_ID",
    "APPLE_NOTARY_KEY_ID",
    "APPLE_NOTARY_KEY_P8",
    "RSNAP_SPARKLE_PRIVATE_ED_KEY",
    "GITHUB_ENV",
    "GITHUB_OUTPUT",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
)
leaked = [name for name in forbidden if name in os.environ]
if leaked:
    raise SystemExit(f"key verifier received forbidden environment values: {leaked}")
if sys.stdin.read().strip() != "fixture-private-key":
    raise SystemExit("key verifier did not receive private key on standard input")
""",
	)
	fake_sign = tool(
		"sign",
		"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["sign", *sys.argv[1:]]) + "\\n")
""",
	)
	fake_validator = tool(
		"validator",
		"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

forbidden = (
    "APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
    "APPLE_NOTARY_ISSUER_ID",
    "APPLE_NOTARY_KEY_ID",
    "APPLE_NOTARY_KEY_P8",
    "RSNAP_SPARKLE_PRIVATE_ED_KEY",
    "GITHUB_ENV",
    "GITHUB_OUTPUT",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
)
leaked = [name for name in forbidden if name in os.environ]
if leaked:
    raise SystemExit(f"artifact validator received forbidden environment values: {leaked}")
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["validator", *sys.argv[1:]]) + "\\n")
""",
	)
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
		f"""#!/usr/bin/env python3
import json, os, sys
import plistlib
from pathlib import Path
args = sys.argv[1:]
forbidden = (
    "APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
    "APPLE_NOTARY_ISSUER_ID",
    "APPLE_NOTARY_KEY_ID",
    "APPLE_NOTARY_KEY_P8",
    "RSNAP_SPARKLE_PRIVATE_ED_KEY",
    "GITHUB_ENV",
    "GITHUB_OUTPUT",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
)
leaked = [name for name in forbidden if name in os.environ]
if leaked:
    raise SystemExit(f"ditto received forbidden environment values: {{leaked}}")
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["ditto", *args]) + "\\n")
if "-x" in args:
    app = Path(args[-1]) / "Rsnap.app"
    (app / "Contents/MacOS").mkdir(parents=True)
    (app / "Contents/MacOS/RsnapNativeHost").write_bytes(b"mach-o")
    (app / "Contents/Info.plist").write_bytes(
        plistlib.dumps({{"SUPublicEDKey": "{PUBLIC_KEY}"}})
    )
else:
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
if os.environ.get("RSNAP_SPARKLE_PRIVATE_ED_KEY") != "fixture-private-key":
    raise SystemExit("appcast did not receive the Rsnap Sparkle private key")
if os.environ.get("RSNAP_SPARKLE_PUBLIC_ED_KEY") is None:
    raise SystemExit("appcast did not receive the embedded Sparkle public key")
forbidden = (
    "APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
    "APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
    "APPLE_NOTARY_ISSUER_ID",
    "APPLE_NOTARY_KEY_ID",
    "APPLE_NOTARY_KEY_P8",
    "GITHUB_ENV",
    "GITHUB_OUTPUT",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
)
leaked = [name for name in forbidden if name in os.environ]
if leaked:
    raise SystemExit(f"appcast received forbidden environment values: {leaked}")
with Path(os.environ["RELEASE_TOOL_LOG"]).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(["appcast", *args]) + "\\n")
Path(args[args.index("--appcast") + 1]).write_text("<rss/>", encoding="utf-8")
""",
	)
	fake_sparkle_signer = tool("sparkle-signer", "#!/bin/sh\nexit 1\n")

	env = os.environ.copy()
	for key in ("SPARKLE_PRIVATE_ED_KEY", "SPARKLE_SIGN_UPDATE"):
		env.pop(key, None)
	env.update(
		{
			"APPLE_DEVELOPER_ID_APPLICATION_IDENTITY": (
				"Developer ID Application: Rsnap Release (TEAM123)"
			),
			"APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64": base64.b64encode(b"cert").decode(),
			"APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD": "fixture-password",
			"APPLE_NOTARY_ISSUER_ID": "12345678-1234-1234-1234-123456789abc",
			"APPLE_NOTARY_KEY_ID": "FIXTURE123",
			"APPLE_NOTARY_KEY_P8": "fixture-p8",
			"BASH_ENV": "/dev/null",
			"ENV": "/dev/null",
			"GITHUB_ENV": str(fixture_root / "github-env"),
			"GITHUB_OUTPUT": str(fixture_root / "github-output"),
			"GITHUB_PATH": str(fixture_root / "github-path"),
			"GITHUB_STEP_SUMMARY": str(fixture_root / "github-summary"),
			"RELEASE_TOOL_LOG": str(log_path),
			"RSNAP_APPCAST_BIN": str(fake_appcast),
			"RSNAP_ARTIFACT_VALIDATOR_BIN": str(fake_validator),
			"RSNAP_CODESIGN_BIN": str(fake_codesign),
			"RSNAP_DITTO_BIN": str(fake_ditto),
			"RSNAP_PYTHON_BIN": sys.executable,
			"RSNAP_RELEASE_COMMIT": SOURCE_COMMIT,
			"RSNAP_RELEASE_OUTPUT_DIR": str(output),
			"RSNAP_RELEASE_TAG": "v1.2.3",
			"RSNAP_RELEASE_VERSION": "1.2.3",
			"RSNAP_SECURITY_BIN": str(fake_security),
			"RSNAP_SIGN_APP_BIN": str(fake_sign),
			"RSNAP_SPARKLE_PRIVATE_ED_KEY": "fixture-private-key",
			"RSNAP_SPARKLE_SIGNER_BIN": str(fake_sparkle_signer),
			"RSNAP_SPARKLE_VERSION": "2.9.4",
			"RSNAP_SPCTL_BIN": str(fake_spctl),
			"RSNAP_UNSIGNED_APP_ARCHIVE": str(handoff_archive),
			"RSNAP_UNSIGNED_APP_ARCHIVE_SHA256": handoff_digest,
			"RSNAP_UNSIGNED_MANIFEST": str(handoff_manifest),
			"RSNAP_UNAME_BIN": str(fake_uname),
			"RSNAP_VERIFY_SPARKLE_KEY_BIN": str(fake_key_verifier),
			"RSNAP_XCRUN_BIN": str(fake_xcrun),
			"RUNNER_ARCH": "ARM64",
			"RUNNER_TEMP": str(runner_temp),
		}
	)
	package_script = RELEASE_DIR / "package-macos.sh"

	extra_handoff_file = handoff_archive.parent / "unexpected"
	extra_handoff_file.write_text("unexpected", encoding="utf-8")
	expect_failure([package_script], env=env)
	assert not log_path.exists()
	extra_handoff_file.unlink()

	for invalid_key_id in ("SHORT", "../../escape", "FIXTURE12\n"):
		invalid_key_env = dict(env)
		invalid_key_env["APPLE_NOTARY_KEY_ID"] = invalid_key_id
		expect_failure([package_script], env=invalid_key_env)
		assert not log_path.exists()
	invalid_issuer_env = dict(env)
	invalid_issuer_env["APPLE_NOTARY_ISSUER_ID"] = "not-a-uuid"
	expect_failure([package_script], env=invalid_issuer_env)
	assert not log_path.exists()
	forbidden_signer_env = dict(env)
	forbidden_signer_env["SPARKLE_SIGN_UPDATE"] = "/tmp/dependency-signer"
	expect_failure([package_script], env=forbidden_signer_env)
	assert not log_path.exists()

	run([package_script], env=env)
	assert (output / ARCHIVE_NAME).is_file()
	assert (output / APPCAST_NAME).is_file()
	assert (output / CHECKSUM_NAME).is_file()
	calls = [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines()]
	tool_names = [call[0] for call in calls]
	assert "build" not in tool_names
	assert tool_names[0] == "validator"
	assert "--source-commit" in calls[0]
	assert calls[0][calls[0].index("--source-commit") + 1] == SOURCE_COMMIT
	assert "--unsigned-archive" in calls[0]
	assert "--unsigned-manifest" in calls[0]
	extract_index = next(
		index for index, call in enumerate(calls) if call[0] == "ditto" and "-x" in call
	)
	assert tool_names.index("sign") < tool_names.index("xcrun")
	assert extract_index < tool_names.index("sign")
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
		RELEASE_DIR / "build-unsigned-macos.sh",
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
		RELEASE_DIR / "validate-release-order.py",
		RELEASE_DIR / "validate-release-source.py",
	):
		compile(script.read_text(encoding="utf-8"), str(script), "exec")

	release_workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
	language_workflow = (ROOT / ".github/workflows/language.yml").read_text(encoding="utf-8")
	release_spec = (ROOT / "docs/spec/release-distribution.md").read_text(encoding="utf-8")
	release_runbook = (ROOT / "docs/runbook/validate-release.md").read_text(encoding="utf-8")
	infisical_config = json.loads((ROOT / ".infisical.json").read_text(encoding="utf-8"))
	secret_topology = json.loads(
		(ROOT / "docs/spec/release-secret-topology.json").read_text(encoding="utf-8")
	)
	publisher_script = (RELEASE_DIR / "publish-github-release.sh").read_text(
		encoding="utf-8"
	)
	unsigned_build_script = (RELEASE_DIR / "build-unsigned-macos.sh").read_text(
		encoding="utf-8"
	)
	package_script = (RELEASE_DIR / "package-macos.sh").read_text(encoding="utf-8")
	appcast_script = (RELEASE_DIR / "sparkle-appcast.sh").read_text(encoding="utf-8")
	sparkle_signer = (RELEASE_DIR / "sign-sparkle-update.swift").read_text(
		encoding="utf-8"
	)
	artifact_validator = (RELEASE_DIR / "validate-release-artifacts.py").read_text(
		encoding="utf-8"
	)
	sparkle_smoke = (ROOT / "scripts/smoke/sparkle-update-local.sh").read_text(
		encoding="utf-8"
	)
	build_job = release_workflow.split("\n  build-macos:", 1)[1].split(
		"\n  sign-macos:", 1
	)[0]
	sign_job = release_workflow.split("\n  sign-macos:", 1)[1].split(
		"\n  publish-release:", 1
	)[0]
	publish_job = release_workflow.split("\n  publish-release:", 1)[1]
	assert "workflow_dispatch" not in release_workflow
	assert "Release Preparation" not in release_workflow
	assert "Release Prep" not in release_workflow
	assert "Release Dry Run" not in release_workflow
	assert "permissions:\n  contents: read" in release_workflow
	assert release_workflow.count("contents: write") == 1
	assert release_workflow.count("name: release") == 2
	assert release_workflow.count("runs-on: macos-26") == 2
	assert "needs: validate-release" in release_workflow
	assert "needs: [validate-release, build-macos]" in release_workflow
	assert "needs: [validate-release, sign-macos]" in publish_job
	assert "environment:" not in build_job
	assert "${{ secrets." not in build_job
	assert "cargo make test-macos-release" in build_job
	assert "scripts/release/build-unsigned-macos.sh" in build_job
	assert "unsigned_artifact_id:" in build_job
	assert "artifact-id" in build_job
	assert "environment:\n      name: release" in sign_job
	for forbidden_build_action in (
		"cargo make",
		"cargo build",
		"swift build",
		"swift package",
		"setup-rust-toolchain",
		"install-action",
		"build_and_run.sh",
		"test-macos-release",
	):
		assert forbidden_build_action not in sign_job
	assert "BASH_ENV: /dev/null" in sign_job
	assert "ENV: /dev/null" in sign_job
	assert "artifact-ids: ${{ needs.build-macos.outputs.unsigned_artifact_id }}" in sign_job
	assert "digest-mismatch: error" in sign_job
	assert "RSNAP_UNSIGNED_APP_ARCHIVE_SHA256:" in sign_job
	assert "RSNAP_UNSIGNED_MANIFEST:" in sign_job
	assert (
		"RSNAP_SPARKLE_PRIVATE_ED_KEY: ${{ secrets.RSNAP_SPARKLE_PRIVATE_ED_KEY }}"
		in sign_job
	)
	for required_secret in (
		"APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
		"APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
		"APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
		"APPLE_NOTARY_ISSUER_ID",
		"APPLE_NOTARY_KEY_ID",
		"APPLE_NOTARY_KEY_P8",
		"RSNAP_SPARKLE_PRIVATE_ED_KEY",
	):
		assert f"secrets.{required_secret}" in sign_job
	assert "${{ secrets.SPARKLE_PRIVATE_ED_KEY }}" not in release_workflow
	assert re.search(r"^\s+SPARKLE_PRIVATE_ED_KEY:", release_workflow, re.MULTILINE) is None
	assert "RSNAP_BUILD_AND_RUN_BIN" not in package_script
	assert "scripts/build_and_run.sh" not in package_script
	assert "SPARKLE_SIGN_UPDATE" in package_script
	assert "unset GITHUB_ENV GITHUB_OUTPUT GITHUB_PATH GITHUB_STEP_SUMMARY" in package_script
	assert "^[A-Z0-9]{10}$" in package_script
	assert "--source-commit" in package_script
	assert "--unsigned-manifest" in package_script
	assert "unset BASH_ENV ENV GITHUB_ENV GITHUB_OUTPUT GITHUB_PATH GITHUB_STEP_SUMMARY" in (
		unsigned_build_script
	)
	assert "status --porcelain --untracked-files=all" in unsigned_build_script
	assert "rev-parse 'HEAD^{commit}'" in unsigned_build_script
	assert "rsnap-unsigned-macos-handoff/1" in unsigned_build_script
	assert "SPARKLE_SIGN_UPDATE" not in appcast_script
	assert "sign-sparkle-update.swift" in appcast_script
	for signer_contract in (
		"import CryptoKit",
		"Curve25519.Signing.PrivateKey",
		"signature(for:",
		"isValidSignature",
	):
		assert signer_contract in sparkle_signer
	assert "rsnap-unsigned-macos-handoff/1" in artifact_validator
	assert "validate_unsigned_manifest" in artifact_validator
	assert "EXPECTED_APP_SYMLINKS" in artifact_validator
	assert "EXPECTED_APP_EXECUTABLES" in artifact_validator
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
	assert set(secret_topology) == {
		"schemaVersion",
		"project",
		"defaults",
		"profiles",
		"identities",
		"accessBindings",
		"exceptions",
	}
	assert secret_topology["schemaVersion"] == 2
	assert secret_topology["project"] == {
		"name": "rsnap-release",
		"provider": "infisical",
		"domain": infisical_config["domain"],
		"projectId": infisical_config["workspaceId"],
	}
	assert secret_topology["defaults"] == {
		"imports": False,
		"recursive": False,
		"expansion": False,
		"overriding": False,
		"broadParentGrant": False,
	}
	profiles = {profile["id"]: profile for profile in secret_topology["profiles"]}
	assert set(profiles) == {"apple-release", "rsnap-sparkle"}
	for profile in profiles.values():
		assert set(profile) == {
			"id",
			"class",
			"path",
			"environments",
			"owner",
			"consumers",
			"requiredKeys",
		}
	assert profiles["apple-release"]["path"] == "/release/apple"
	assert profiles["apple-release"]["environments"] == [
		infisical_config["defaultEnvironment"]
	]
	assert set(profiles["apple-release"]["requiredKeys"]) == {
		"APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64",
		"APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD",
		"APPLE_DEVELOPER_ID_APPLICATION_IDENTITY",
		"APPLE_NOTARY_KEY_ID",
		"APPLE_NOTARY_ISSUER_ID",
		"APPLE_NOTARY_KEY_P8",
	}
	assert profiles["rsnap-sparkle"]["path"] == "/release/sparkle"
	assert profiles["rsnap-sparkle"]["environments"] == [
		infisical_config["defaultEnvironment"]
	]
	assert profiles["rsnap-sparkle"]["requiredKeys"] == [
		"RSNAP_SPARKLE_PRIVATE_ED_KEY"
	]
	identities = {
		identity["id"]: identity for identity in secret_topology["identities"]
	}
	assert set(identities) == {"rsnap-release-provisioner"}
	assert set(identities["rsnap-release-provisioner"]) == {
		"id",
		"purpose",
		"owner",
		"trustBoundary",
		"auth",
		"delivery",
		"credentialLifecycle",
	}
	assert identities["rsnap-release-provisioner"]["auth"] == {
		"method": "universal-auth",
		"selector": "identity:ac5f2cd1-4cfa-49a5-a9f4-9534a6111138",
	}
	bindings = secret_topology["accessBindings"]
	for binding in bindings:
		assert set(binding) == {"identity", "profile", "environments", "actions"}
	assert {
		(binding["identity"], binding["profile"])
		for binding in bindings
	} == {
		("rsnap-release-provisioner", "apple-release"),
		("rsnap-release-provisioner", "rsnap-sparkle"),
	}
	for binding in bindings:
		assert binding["environments"] == [infisical_config["defaultEnvironment"]]
		assert binding["actions"] == ["read", "create", "edit", "delete"]
	assert secret_topology["exceptions"] == [
		{
			"id": "dedicated-project-member-grant",
			"rule": "defaults.broadParentGrant",
			"owner": "acgxv",
			"approver": "acgxv",
			"reason": (
				"The provisioner has project-member access because the dedicated project "
				"contains only the two declared Rsnap release profiles"
			),
			"expiresAt": "2027-01-26T00:00:00Z",
		}
	]
	for release_document in (release_spec, release_runbook):
		normalized_release_document = " ".join(release_document.split())
		assert "`/release/apple`" in normalized_release_document
		assert "`/release/sparkle`" in normalized_release_document
		assert "`rsnap-release-provisioner`" in normalized_release_document
	assert "--verify-appcast-signature" in publisher_script
	assert "releases/tags/" not in publisher_script
	assert "release list" in publisher_script
	assert "tagName,isDraft,isPrerelease,isLatest" in publisher_script
	assert "make_latest=legacy" in publisher_script
	assert "make_latest=true" not in publisher_script
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
		test_release_order_validator(tmp)
		test_publisher(tmp)
		test_unsigned_build_orchestrator(tmp)
		test_package_orchestrator(tmp)
	test_static_contracts()
	print("release self-check passed")
	return 0


if __name__ == "__main__":
	raise SystemExit(main())
