#!/usr/bin/env bash
set -euo pipefail

ARCHIVE_NAME="rsnap-aarch64-apple-darwin.zip"
APPCAST_NAME="appcast.xml"
CHECKSUM_NAME="${ARCHIVE_NAME}.sha256"
CANONICAL_REPOSITORY="acg-box/rsnap"

die() {
	echo "error: $*" >&2
	exit 1
}

for required_variable in \
	GH_TOKEN \
	GITHUB_REPOSITORY \
	GITHUB_SHA \
	RSNAP_RELEASE_COMMIT \
	RSNAP_RELEASE_TAG \
	RSNAP_RELEASE_VERSION \
	RSNAP_RELEASE_INPUT_DIR; do
	if [[ -z "${!required_variable:-}" ]]; then
		die "missing required environment variable: ${required_variable}"
	fi
done

if [[ "$GITHUB_REPOSITORY" != "$CANONICAL_REPOSITORY" ]]; then
	die "GITHUB_REPOSITORY must be ${CANONICAL_REPOSITORY}"
fi
if [[ "$GITHUB_SHA" != "$RSNAP_RELEASE_COMMIT" ]]; then
	die "GITHUB_SHA does not match RSNAP_RELEASE_COMMIT"
fi
if [[ ! -d "$RSNAP_RELEASE_INPUT_DIR" ]]; then
	die "release input directory does not exist: ${RSNAP_RELEASE_INPUT_DIR}"
fi

input_dir="$(cd "$RSNAP_RELEASE_INPUT_DIR" && pwd -P)"
archive="${input_dir}/${ARCHIVE_NAME}"
appcast="${input_dir}/${APPCAST_NAME}"
checksum="${input_dir}/${CHECKSUM_NAME}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
validator="${RSNAP_RELEASE_VALIDATOR:-${script_dir}/validate-release-artifacts.py}"
gh_bin="${GH_BIN:-gh}"

if [[ ! -x "$validator" ]]; then
	die "release validator is not executable: ${validator}"
fi
if ! command -v "$gh_bin" >/dev/null 2>&1; then
	die "GitHub CLI is not available: ${gh_bin}"
fi

run_validator_for_paths() {
	local validation_archive="$1"
	local validation_appcast="$2"
	local validation_checksum="$3"
	shift 3
	"$validator" \
		--archive "$validation_archive" \
		--appcast "$validation_appcast" \
		--checksum "$validation_checksum" \
		--version "$RSNAP_RELEASE_VERSION" \
		--tag "$RSNAP_RELEASE_TAG" \
		--repository "$GITHUB_REPOSITORY" \
		--verify-appcast-signature \
		"$@"
}

run_validator() {
	run_validator_for_paths "$archive" "$appcast" "$checksum" "$@"
}

fetch_assets() {
	local release_id="$1"
	local output_path="$2"
	local pages_path="${output_path}.pages"
	"$gh_bin" api \
		"repos/${GITHUB_REPOSITORY}/releases/${release_id}/assets?per_page=100" \
		--paginate \
		--slurp > "$pages_path"
	python3 -c '
import json
import pathlib
import sys

pages = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if not isinstance(pages, list) or any(not isinstance(page, list) for page in pages):
    raise SystemExit("GitHub paginated assets response must contain arrays")
json.dump([asset for page in pages for asset in page], sys.stdout)
	' "$pages_path" > "$output_path"
}

fetch_releases() {
	local output_path="$1"
	local pages_path="${output_path}.pages"
	"$gh_bin" api \
		"repos/${GITHUB_REPOSITORY}/releases?per_page=100" \
		--paginate \
		--slurp > "$pages_path"
	python3 -c '
import json
import pathlib
import sys

pages = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if not isinstance(pages, list) or any(not isinstance(page, list) for page in pages):
    raise SystemExit("GitHub paginated releases response must contain arrays")
json.dump([release for page in pages for release in page], sys.stdout)
' "$pages_path" > "$output_path"
}

json_object_field() {
	local metadata_path="$1"
	local field_name="$2"
	python3 -c '
import json
import pathlib
import sys

value = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")).get(sys.argv[2])
if isinstance(value, bool):
    print("true" if value else "false")
elif isinstance(value, (int, str)):
    print(value)
else:
    raise SystemExit(f"invalid JSON metadata field: {sys.argv[2]}")
' "$metadata_path" "$field_name"
}

validate_monotonic_version() {
	local metadata_path="$1"
	python3 -c '
import json
import pathlib
import re
import sys

releases = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if not isinstance(releases, list):
    raise SystemExit("GitHub releases response must be an array")

pattern = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
target = tuple(int(part) for part in sys.argv[2].split("."))
stable_versions = []
for release in releases:
    if release.get("draft") is not False or release.get("prerelease") is not False:
        continue
    match = pattern.fullmatch(str(release.get("tag_name", "")))
    if match is not None:
        stable_versions.append(tuple(int(part) for part in match.groups()))

if stable_versions and target <= max(stable_versions):
    highest = ".".join(str(part) for part in max(stable_versions))
    raise SystemExit(
        f"release version {sys.argv[2]} must be higher than published stable v{highest}"
    )
	' "$metadata_path" "$RSNAP_RELEASE_VERSION"
}

download_assets() {
	local download_dir="$1"
	local compare_with_local="$2"
	local asset_id asset_name local_path
	mkdir -p "$download_dir"
	while IFS= read -r -d '' asset_id && IFS= read -r -d '' asset_name; do
		case "$asset_name" in
			"$ARCHIVE_NAME")
				local_path="$archive"
				;;
			"$APPCAST_NAME")
				local_path="$appcast"
				;;
			"$CHECKSUM_NAME")
				local_path="$checksum"
				;;
			*)
				die "unexpected asset in GitHub metadata: ${asset_name}"
				;;
		esac
		"$gh_bin" api \
			-H "Accept: application/octet-stream" \
			"repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" \
			> "${download_dir}/${asset_name}"
		if [[ "$compare_with_local" == "true" ]] \
			&& ! cmp -s "$local_path" "${download_dir}/${asset_name}"; then
			die "downloaded GitHub asset does not match local bytes: ${asset_name}"
		fi
	done < <(
		python3 -c '
import json
import pathlib
import sys

assets = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
if not isinstance(assets, list):
    raise SystemExit("GitHub assets response must be an array")
for asset in assets:
    if not isinstance(asset, dict):
        raise SystemExit("GitHub asset metadata entry must be an object")
    asset_id = asset.get("id")
    name = asset.get("name")
    if not isinstance(asset_id, int) or asset_id <= 0 or not isinstance(name, str):
        raise SystemExit("GitHub asset metadata has an invalid ID or name")
    sys.stdout.write(str(asset_id) + "\0" + name + "\0")
		' "$assets_json"
	)
}

for local_artifact in "$archive" "$appcast" "$checksum"; do
	if [[ ! -f "$local_artifact" ]]; then
		die "release artifact does not exist: ${local_artifact}"
	fi
done

# No GitHub release is created until all local bytes and signatures pass.
run_validator

temp_dir="$(mktemp -d "${RUNNER_TEMP:-/tmp}/rsnap-release-publish.XXXXXX")"
trap 'rm -rf "$temp_dir"' EXIT
release_json="${temp_dir}/release.json"
assets_json="${temp_dir}/assets.json"
api_error="${temp_dir}/api-error.txt"
stable_releases_json="${temp_dir}/stable-releases.json"
tag_commit_json="${temp_dir}/tag-commit.json"

"$gh_bin" api \
	"repos/${GITHUB_REPOSITORY}/commits/${RSNAP_RELEASE_TAG}" \
	> "$tag_commit_json"
tag_commit="$(json_object_field "$tag_commit_json" sha)"
if [[ "$tag_commit" != "$RSNAP_RELEASE_COMMIT" ]]; then
	die "remote release tag does not resolve to RSNAP_RELEASE_COMMIT"
fi

release_was_created=0
release_exists=1
if "$gh_bin" api \
	"repos/${GITHUB_REPOSITORY}/releases/tags/${RSNAP_RELEASE_TAG}" \
	> "$release_json" 2> "$api_error"; then
	:
elif grep -Eq 'HTTP 404|Not Found' "$api_error"; then
	release_exists=0
else
	cat "$api_error" >&2
	die "cannot query the GitHub release"
fi

if [[ "$release_exists" == "1" ]]; then
	release_is_draft="$(json_object_field "$release_json" draft)"
else
	release_is_draft=true
fi

# A published release with this exact tag is the idempotent exception. New and
# draft releases must advance the highest visible stable semantic version.
if [[ "$release_is_draft" == "true" ]]; then
	fetch_releases "$stable_releases_json"
	validate_monotonic_version "$stable_releases_json"
fi

if [[ "$release_exists" == "0" ]]; then
	"$gh_bin" api \
		--method POST \
		"repos/${GITHUB_REPOSITORY}/releases" \
		-f "tag_name=${RSNAP_RELEASE_TAG}" \
		-f "target_commitish=${RSNAP_RELEASE_COMMIT}" \
		-F draft=true \
		-F prerelease=false \
		-F generate_release_notes=true > "$release_json"
	release_was_created=1
fi

release_id="$(json_object_field "$release_json" id)"
release_is_draft="$(json_object_field "$release_json" draft)"
case "$release_is_draft" in
	true)
		run_validator \
			--release-json "$release_json" \
			--release-state draft
		;;
	false)
		fetch_assets "$release_id" "$assets_json"
		published_dir="${temp_dir}/published"
		download_assets "$published_dir" false
		run_validator_for_paths \
			"$published_dir/$ARCHIVE_NAME" \
			"$published_dir/$APPCAST_NAME" \
			"$published_dir/$CHECKSUM_NAME" \
			--release-json "$release_json" \
			--assets-json "$assets_json" \
			--release-state published
		echo "GitHub release ${RSNAP_RELEASE_TAG} is already public and its remote assets validate."
		exit 0
		;;
	*)
		die "GitHub release draft state is invalid"
		;;
esac

# A reused draft can contain partial files from an earlier failed attempt. Remove
# all resolved draft assets, then upload one exact set. A failure still leaves an
# invisible draft that this script can repair on the next run.
fetch_assets "$release_id" "$assets_json"
while IFS= read -r -d '' asset_id; do
	"$gh_bin" api \
		--method DELETE \
		"repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" \
		--silent
done < <(
	python3 -c '
import json
import pathlib
import sys

assets = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
for asset in assets:
    sys.stdout.write(str(asset["id"]) + "\0")
	' "$assets_json"
)

"$gh_bin" release upload "$RSNAP_RELEASE_TAG" \
	--repo "$GITHUB_REPOSITORY" \
	"$archive" \
	"$appcast" \
	"$checksum"

"$gh_bin" api \
	"repos/${GITHUB_REPOSITORY}/releases/tags/${RSNAP_RELEASE_TAG}" \
	> "$release_json"
fetch_assets "$release_id" "$assets_json"
run_validator \
	--release-json "$release_json" \
	--assets-json "$assets_json" \
	--release-state draft
download_assets "${temp_dir}/download" true

if [[ "$release_was_created" == "1" ]]; then
	echo "Created and validated draft release ${RSNAP_RELEASE_TAG}."
else
	echo "Reused and validated draft release ${RSNAP_RELEASE_TAG}."
fi

# Publishing is the last GitHub operation. All metadata checks and remote byte
# downloads have completed, so an earlier failure cannot expose a partial release.
"$gh_bin" api \
	--method PATCH \
	"repos/${GITHUB_REPOSITORY}/releases/${release_id}" \
	-F draft=false \
	-f make_latest=true \
	--silent
echo "Published ${RSNAP_RELEASE_TAG} as the latest stable GitHub release."
