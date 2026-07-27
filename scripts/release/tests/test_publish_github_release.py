from __future__ import annotations

import json
import os
import pathlib
import stat
import subprocess
import tempfile
import textwrap
import unittest


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[3]
PUBLISHER_PATH = REPOSITORY_ROOT / "scripts/release/publish-github-release.py"
ARCHIVE_NAME = "rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME = "appcast.xml"
CHECKSUM_NAME = f"{ARCHIVE_NAME}.sha256"
COMMIT = "a" * 40


FAKE_GH = r"""#!/usr/bin/env python3
import json
import os
import pathlib
import shutil
import sys

state_dir = pathlib.Path(os.environ["MOCK_GH_STATE"])
state_dir.mkdir(parents=True, exist_ok=True)
release_path = state_dir / "release.json"
assets_dir = state_dir / "assets"
assets_dir.mkdir(exist_ok=True)
log_path = state_dir / "commands.jsonl"
with log_path.open("a", encoding="utf-8") as log:
    log.write(json.dumps(sys.argv[1:]) + "\n")

args = sys.argv[1:]
if not args:
    raise SystemExit(2)

def release_metadata(draft=True):
    download_tag = (
        "untagged-01234567-89ab-cdef-0123-456789abcdef"
        if draft
        else "v1.2.3"
    )
    return {
        "id": 42,
        "tag_name": "v1.2.3",
        "target_commitish": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "draft": draft,
        "prerelease": False,
        "url": "https://api.github.com/repos/acg-box/rsnap/releases/42",
        "html_url": f"https://github.com/acg-box/rsnap/releases/tag/{download_tag}",
    }

def load_release():
    return json.loads(release_path.read_text(encoding="utf-8"))

def write_release(metadata):
    release_path.write_text(json.dumps(metadata), encoding="utf-8")

def asset_metadata():
    release = load_release()
    download_tag = release["html_url"].rsplit("/", 1)[1]
    result = []
    for asset_id, path in enumerate(sorted(assets_dir.iterdir()), start=101):
        result.append({
            "id": asset_id,
            "name": path.name,
            "size": path.stat().st_size,
            "state": "uploaded",
            "url": (
                "https://api.github.com/repos/acg-box/rsnap/releases/assets/"
                f"{asset_id}"
            ),
            "browser_download_url": (
                f"https://github.com/acg-box/rsnap/releases/download/{download_tag}/"
                f"{path.name}"
            ),
        })
    return result

if args[0] == "release" and args[1] == "upload":
    repo_index = args.index("--repo")
    artifact_args = args[repo_index + 2:]
    for source in artifact_args:
        source_path = pathlib.Path(source)
        shutil.copyfile(source_path, assets_dir / source_path.name)
    raise SystemExit(0)

if args[0] != "api":
    raise SystemExit(2)

method = "GET"
if "--method" in args:
    method = args[args.index("--method") + 1]
endpoint = next(
    value for value in args
    if value.startswith("repos/")
)

if endpoint == "repos/acg-box/rsnap/releases/tags/v1.2.3":
    if not release_path.exists():
        print("gh: Not Found (HTTP 404)", file=sys.stderr)
        raise SystemExit(1)
    print(json.dumps(load_release()))
elif endpoint == "repos/acg-box/rsnap/commits/v1.2.3":
    print(json.dumps({"sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}))
elif endpoint.startswith("repos/acg-box/rsnap/releases?per_page=100&page="):
    page = int(endpoint.rsplit("=", 1)[1])
    versions = [
        version
        for version in os.environ.get("MOCK_PUBLISHED_VERSIONS", "1.2.2").split(",")
        if version
    ]
    releases = [
        {
            "tag_name": f"v{version}",
            "draft": False,
            "prerelease": False,
        }
        for version in versions
    ]
    start = (page - 1) * 100
    print(json.dumps(releases[start:start + 100]))
elif endpoint == "repos/acg-box/rsnap/releases" and method == "POST":
    metadata = release_metadata()
    write_release(metadata)
    print(json.dumps(metadata))
elif endpoint == "repos/acg-box/rsnap/releases/42/assets?per_page=100":
    print(json.dumps(asset_metadata()))
elif endpoint.startswith("repos/acg-box/rsnap/releases/assets/") and method == "DELETE":
    asset_id = int(endpoint.rsplit("/", 1)[1])
    assets = asset_metadata()
    matching = [asset for asset in assets if asset["id"] == asset_id]
    if matching:
        (assets_dir / matching[0]["name"]).unlink()
elif endpoint.startswith("repos/acg-box/rsnap/releases/assets/"):
    asset_id = int(endpoint.rsplit("/", 1)[1])
    matching = [asset for asset in asset_metadata() if asset["id"] == asset_id]
    if not matching:
        raise SystemExit(1)
    data = (assets_dir / matching[0]["name"]).read_bytes()
    if os.environ.get("MOCK_CORRUPT_DOWNLOAD") == "1" and data:
        data = bytes([data[0] ^ 1]) + data[1:]
    sys.stdout.buffer.write(data)
elif endpoint == "repos/acg-box/rsnap/releases/42" and method == "PATCH":
    metadata = load_release()
    metadata["draft"] = False
    metadata["html_url"] = "https://github.com/acg-box/rsnap/releases/tag/v1.2.3"
    write_release(metadata)
else:
    print(f"unsupported fake gh call: {args}", file=sys.stderr)
    raise SystemExit(2)
"""


class PublisherTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary_directory.name)
        self.input_dir = self.root / "input"
        self.input_dir.mkdir()
        (self.input_dir / ARCHIVE_NAME).write_bytes(b"archive bytes")
        (self.input_dir / APPCAST_NAME).write_bytes(b"appcast bytes")
        (self.input_dir / CHECKSUM_NAME).write_bytes(b"checksum bytes")

        self.validator = self.root / "validator"
        self.validator.write_text(
            textwrap.dedent(
                """\
                #!/usr/bin/env bash
                set -euo pipefail
                printf '%s\\n' "$*" >> "$MOCK_VALIDATOR_LOG"
                if [[ "${MOCK_VALIDATOR_FAIL:-0}" == "1" ]]; then
                    exit 1
                fi
                if [[ "${MOCK_VALIDATOR_FAIL_REMOTE:-0}" == "1" ]] \
                  && [[ "$(wc -l < "$MOCK_VALIDATOR_LOG")" -gt 1 ]]; then
                    exit 1
                fi
                """
            ),
            encoding="utf-8",
        )
        self.fake_gh = self.root / "gh"
        self.fake_gh.write_text(FAKE_GH, encoding="utf-8")
        for executable in (self.validator, self.fake_gh):
            executable.chmod(executable.stat().st_mode | stat.S_IXUSR)

        self.state_dir = self.root / "gh-state"
        self.validator_log = self.root / "validator.log"
        self.environment = {
            **os.environ,
            "GH_TOKEN": "test-token",
            "GITHUB_REPOSITORY": "acg-box/rsnap",
            "GITHUB_SHA": COMMIT,
            "RSNAP_RELEASE_COMMIT": COMMIT,
            "RSNAP_RELEASE_TAG": "v1.2.3",
            "RSNAP_RELEASE_VERSION": "1.2.3",
            "RSNAP_RELEASE_INPUT_DIR": str(self.input_dir),
            "RSNAP_RELEASE_VALIDATOR": str(self.validator),
            "GH_BIN": str(self.fake_gh),
            "MOCK_GH_STATE": str(self.state_dir),
            "MOCK_VALIDATOR_LOG": str(self.validator_log),
        }
        self.run_count = 0

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def run_publisher(
        self,
        *,
        extra_environment: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        environment = dict(self.environment)
        if extra_environment:
            environment.update(extra_environment)
        self.run_count += 1
        stdout_path = self.root / f"publisher-{self.run_count}.stdout"
        stderr_path = self.root / f"publisher-{self.run_count}.stderr"
        arguments = [str(PUBLISHER_PATH)]
        with stdout_path.open("w", encoding="utf-8") as stdout_file:
            with stderr_path.open("w", encoding="utf-8") as stderr_file:
                result = subprocess.run(
                    arguments,
                    check=False,
                    stdout=stdout_file,
                    stderr=stderr_file,
                    text=True,
                    env=environment,
                )
        return subprocess.CompletedProcess(
            arguments,
            result.returncode,
            stdout_path.read_text(encoding="utf-8"),
            stderr_path.read_text(encoding="utf-8"),
        )

    def commands(self) -> list[list[str]]:
        log_path = self.state_dir / "commands.jsonl"
        if not log_path.exists():
            return []
        return [
            json.loads(line)
            for line in log_path.read_text(encoding="utf-8").splitlines()
        ]

    def test_publishes_only_after_remote_bytes_match(self) -> None:
        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        release = json.loads(
            (self.state_dir / "release.json").read_text(encoding="utf-8")
        )
        self.assertFalse(release["draft"])
        self.assertEqual(
            {path.name for path in (self.state_dir / "assets").iterdir()},
            {ARCHIVE_NAME, APPCAST_NAME, CHECKSUM_NAME},
        )
        validator_calls = self.validator_log.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(validator_calls), 1)
        self.assertIn("--archive", validator_calls[0])
        patch_commands = [
            command
            for command in self.commands()
            if command[0:3] == ["api", "--method", "PATCH"]
        ]
        self.assertEqual(len(patch_commands), 1)
        self.assertIn("make_latest=true", patch_commands[0])
        self.assertEqual(self.commands()[-1], patch_commands[0])

    def test_local_validation_failure_does_not_create_a_release(self) -> None:
        result = self.run_publisher(
            extra_environment={"MOCK_VALIDATOR_FAIL": "1"},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.state_dir / "release.json").exists())
        self.assertEqual(self.commands(), [])

    def test_remote_byte_failure_keeps_draft_and_rerun_repairs_it(self) -> None:
        failed = self.run_publisher(
            extra_environment={"MOCK_CORRUPT_DOWNLOAD": "1"},
        )
        self.assertNotEqual(failed.returncode, 0)
        release = json.loads(
            (self.state_dir / "release.json").read_text(encoding="utf-8")
        )
        self.assertTrue(release["draft"])

        repaired = self.run_publisher()
        self.assertEqual(repaired.returncode, 0, repaired.stderr)
        release = json.loads(
            (self.state_dir / "release.json").read_text(encoding="utf-8")
        )
        self.assertFalse(release["draft"])

    def test_monotonic_gate_scans_all_published_releases(self) -> None:
        published_versions = [f"0.0.{patch}" for patch in range(1, 101)]
        published_versions.append("2.0.0")
        result = self.run_publisher(
            extra_environment={
                "MOCK_PUBLISHED_VERSIONS": ",".join(published_versions),
            },
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must be higher", result.stderr)
        self.assertFalse((self.state_dir / "release.json").exists())

    def seed_published_release(self) -> None:
        self.state_dir.mkdir(parents=True)
        (self.state_dir / "assets").mkdir()
        release = {
            "id": 42,
            "tag_name": "v1.2.3",
            "target_commitish": COMMIT,
            "draft": False,
            "prerelease": False,
            "url": "https://api.github.com/repos/acg-box/rsnap/releases/42",
            "html_url": "https://github.com/acg-box/rsnap/releases/tag/v1.2.3",
        }
        (self.state_dir / "release.json").write_text(
            json.dumps(release),
            encoding="utf-8",
        )
        for name in (ARCHIVE_NAME, APPCAST_NAME, CHECKSUM_NAME):
            (self.state_dir / "assets" / name).write_bytes(
                f"published {name}".encode()
            )

    def test_published_release_is_idempotent(self) -> None:
        self.seed_published_release()

        result = self.run_publisher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already public", result.stdout)
        validator_calls = self.validator_log.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(validator_calls), 2)
        self.assertIn("rsnap-release-download-", validator_calls[1])
        self.assertFalse(
            any(command[0:3] == ["api", "--method", "PATCH"] for command in self.commands())
        )

    def test_invalid_published_release_fails(self) -> None:
        self.seed_published_release()

        result = self.run_publisher(
            extra_environment={"MOCK_VALIDATOR_FAIL_REMOTE": "1"},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("artifact validation failed", result.stderr)
        self.assertFalse(
            any(command[0:3] == ["api", "--method", "PATCH"] for command in self.commands())
        )


if __name__ == "__main__":
    unittest.main()
