#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CANONICAL_REPOSITORY="acg-box/rsnap"
UNSIGNED_ARCHIVE_NAME="rsnap-unsigned-aarch64-apple-darwin.zip"
UNSIGNED_MANIFEST_NAME="rsnap-unsigned-aarch64-apple-darwin.json"

required_values=(
	RSNAP_RELEASE_COMMIT
	RSNAP_RELEASE_VERSION
	RSNAP_RELEASE_TAG
	RSNAP_SPARKLE_VERSION
)
for required_value in "${required_values[@]}"; do
	if [[ -z "${!required_value:-}" ]]; then
		echo "error: missing required unsigned-build value: $required_value" >&2
		exit 1
	fi
done

release_secret_names=(
	APPLE_DEVELOPER_ID_APPLICATION_IDENTITY
	APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64
	APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD
	APPLE_NOTARY_ISSUER_ID
	APPLE_NOTARY_KEY_ID
	APPLE_NOTARY_KEY_P8
	RSNAP_SPARKLE_PRIVATE_ED_KEY
)
for secret_name in "${release_secret_names[@]}"; do
	if [[ -n "${!secret_name:-}" ]]; then
		echo "error: unsigned build must not receive release secret: $secret_name" >&2
		exit 1
	fi
done

if [[ "$RSNAP_RELEASE_TAG" != "v$RSNAP_RELEASE_VERSION" ]]; then
	echo "error: release tag and version do not match" >&2
	exit 1
fi
if [[ ! "$RSNAP_RELEASE_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
	echo "error: release commit must be a full lowercase Git object SHA" >&2
	exit 1
fi

runner_arch="${RUNNER_ARCH:-}"
uname_bin="${RSNAP_UNAME_BIN:-/usr/bin/uname}"
if [[ "$runner_arch" != "ARM64" || "$("$uname_bin" -m)" != "arm64" ]]; then
	echo "error: Rsnap release build requires the macos-26 ARM64 runner" >&2
	exit 1
fi

python_bin="${RSNAP_PYTHON_BIN:-$(command -v python3 || true)}"
ditto_bin="${RSNAP_DITTO_BIN:-/usr/bin/ditto}"
git_bin="${RSNAP_GIT_BIN:-/usr/bin/git}"
build_script="${RSNAP_BUILD_AND_RUN_BIN:-$ROOT_DIR/scripts/build_and_run.sh}"
artifact_validator="${RSNAP_ARTIFACT_VALIDATOR_BIN:-$ROOT_DIR/scripts/release/validate-release-artifacts.py}"
for executable in \
	"$python_bin" \
	"$ditto_bin" \
	"$git_bin" \
	"$build_script" \
	"$artifact_validator"; do
	if [[ ! -x "$executable" ]]; then
		echo "error: required unsigned-build tool is not executable: $executable" >&2
		exit 1
	fi
done

runner_temp="${RUNNER_TEMP:-}"
if [[ -z "$runner_temp" || ! -d "$runner_temp" ]]; then
	echo "error: RUNNER_TEMP must name an existing directory" >&2
	exit 1
fi
output_dir="${RSNAP_UNSIGNED_OUTPUT_DIR:-$runner_temp/rsnap-unsigned-output}"
mkdir -p "$output_dir"
archive_path="$output_dir/$UNSIGNED_ARCHIVE_NAME"
manifest_path="$output_dir/$UNSIGNED_MANIFEST_NAME"
rm -f "$archive_path" "$manifest_path"

umask 077
work_root="$(mktemp -d "$runner_temp/rsnap-unsigned-build.XXXXXX")"
stage_dir="$work_root/stage"
build_complete=0

cleanup() {
	if [[ "$build_complete" != "1" ]]; then
		rm -f "$archive_path" "$manifest_path" || true
	fi
	if [[ "$work_root" == "$runner_temp"/rsnap-unsigned-build.* && -d "$work_root" ]]; then
		rm -rf "$work_root" || true
	fi
}
trap cleanup EXIT

# Build tools do not need GitHub command files. Removing these variables prevents a dependency
# process from changing later workflow steps or outputs through the runner command protocol.
github_output="${GITHUB_OUTPUT:-}"
unset BASH_ENV ENV GITHUB_ENV GITHUB_OUTPUT GITHUB_PATH GITHUB_STEP_SUMMARY

verify_source_tree() {
	local actual_commit status
	actual_commit="$("$git_bin" -C "$ROOT_DIR" rev-parse 'HEAD^{commit}')"
	if [[ "$actual_commit" != "$RSNAP_RELEASE_COMMIT" ]]; then
		echo "error: unsigned build source commit changed" >&2
		exit 1
	fi
	status="$("$git_bin" -C "$ROOT_DIR" status --porcelain --untracked-files=all)"
	if [[ -n "$status" ]]; then
		echo "error: unsigned build source tree must remain clean" >&2
		exit 1
	fi
}

verify_source_tree

RSNAP_NATIVE_HOST_APP_VERSION="$RSNAP_RELEASE_VERSION" \
	RSNAP_NATIVE_HOST_FORCE_REBUILD=1 \
	RSNAP_NATIVE_HOST_RUST_PROFILE=final-release \
	RSNAP_NATIVE_HOST_SKIP_SIGNING=1 \
	RSNAP_NATIVE_HOST_STAGE_DIR="$stage_dir" \
	RSNAP_NATIVE_HOST_SWIFT_CLEAN=1 \
	RSNAP_NATIVE_HOST_SWIFT_CONFIGURATION=release \
	"$build_script" stage

verify_source_tree

app_path="$stage_dir/Rsnap.app"
"$artifact_validator" \
	--app "$app_path" \
	--repository "$CANONICAL_REPOSITORY" \
	--sparkle-version "$RSNAP_SPARKLE_VERSION" \
	--tag "$RSNAP_RELEASE_TAG" \
	--version "$RSNAP_RELEASE_VERSION"

# The unsigned handoff archive carries only the validated app. It intentionally omits extended
# attributes, ACLs, quarantine data, and resource forks from the untrusted build runner.
"$ditto_bin" -c -k \
	--norsrc \
	--noextattr \
	--noqtn \
	--noacl \
	--nopersistRootless \
	--keepParent \
	"$app_path" \
	"$archive_path"

"$artifact_validator" \
	--unsigned-archive "$archive_path" \
	--repository "$CANONICAL_REPOSITORY" \
	--sparkle-version "$RSNAP_SPARKLE_VERSION" \
	--tag "$RSNAP_RELEASE_TAG" \
	--version "$RSNAP_RELEASE_VERSION"

archive_sha256="$("$python_bin" - "$archive_path" <<'PY'
import hashlib
import sys

digest = hashlib.sha256()
with open(sys.argv[1], "rb") as handle:
	for chunk in iter(lambda: handle.read(1 << 20), b""):
		digest.update(chunk)
print(digest.hexdigest())
PY
)"
REPOSITORY="$CANONICAL_REPOSITORY" \
	SOURCE_COMMIT="$RSNAP_RELEASE_COMMIT" \
	TAG="$RSNAP_RELEASE_TAG" \
	VERSION="$RSNAP_RELEASE_VERSION" \
	SPARKLE_VERSION="$RSNAP_SPARKLE_VERSION" \
	ARCHIVE_NAME="$UNSIGNED_ARCHIVE_NAME" \
	ARCHIVE_SHA256="$archive_sha256" \
	MANIFEST_PATH="$manifest_path" \
	"$python_bin" - <<'PY'
import json
import os
from pathlib import Path

manifest = {
	"schema": "rsnap-unsigned-macos-handoff/1",
	"repository": os.environ["REPOSITORY"],
	"source_commit": os.environ["SOURCE_COMMIT"],
	"tag": os.environ["TAG"],
	"version": os.environ["VERSION"],
	"sparkle_version": os.environ["SPARKLE_VERSION"],
	"archive": {
		"name": os.environ["ARCHIVE_NAME"],
		"sha256": os.environ["ARCHIVE_SHA256"],
	},
}
Path(os.environ["MANIFEST_PATH"]).write_text(
	json.dumps(manifest, indent=2, sort_keys=True) + "\n",
	encoding="utf-8",
)
PY

"$artifact_validator" \
	--source-commit "$RSNAP_RELEASE_COMMIT" \
	--unsigned-archive "$archive_path" \
	--unsigned-archive-sha256 "$archive_sha256" \
	--unsigned-manifest "$manifest_path" \
	--repository "$CANONICAL_REPOSITORY" \
	--sparkle-version "$RSNAP_SPARKLE_VERSION" \
	--tag "$RSNAP_RELEASE_TAG" \
	--version "$RSNAP_RELEASE_VERSION"
if [[ -n "$github_output" ]]; then
	printf 'archive_sha256=%s\n' "$archive_sha256" >>"$github_output"
fi

build_complete=1
printf 'Prepared unsigned release handoff:\n  %s\n  sha256:%s\n' \
	"$archive_path" \
	"$archive_sha256"
printf '  %s\n' "$manifest_path"
