#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-show}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUNDLE_ID="${RSNAP_TELEMETRY_BUNDLE_ID:-ink.hack.rsnap}"
EXECUTABLE_NAME="${RSNAP_TELEMETRY_PROCESS:-RsnapNativeHost}"
LAST="${RSNAP_TELEMETRY_LAST:-10m}"
RUST_LOG_DIR="${RSNAP_RUST_LOG_DIR:-$HOME/Library/Application Support/ink.hack.rsnap/logs}"
OUT_DIR="${RSNAP_TELEMETRY_OUT_DIR:-$ROOT_DIR/target/telemetry/native-host-$(date +%Y%m%d-%H%M%S)}"
NATIVE_PREDICATE="${RSNAP_TELEMETRY_PREDICATE:-subsystem == \"$BUNDLE_ID\" AND composedMessage CONTAINS \"schema=rsnap.native_host.telemetry/1\"}"

usage() {
	cat <<USAGE
usage: $0 [show|stream|collect|summary]

Environment:
  RSNAP_TELEMETRY_LAST       log show window, default: 10m
  RSNAP_TELEMETRY_OUT_DIR    collect output directory
  RSNAP_TELEMETRY_PREDICATE  macOS log predicate override
  RSNAP_RUST_LOG_DIR         Rust rolling log directory
USAGE
}

write_native_log() {
	local destination="$1"
	/usr/bin/log show --info --style compact --last "$LAST" --predicate "$NATIVE_PREDICATE" \
		>"$destination"
}

last_window_seconds() {
	local value="$LAST"

	if [[ "$value" =~ ^([0-9]+)([smhd])$ ]]; then
		local number="${BASH_REMATCH[1]}"
		local suffix="${BASH_REMATCH[2]}"
		case "$suffix" in
			s)
				printf '%s\n' "$((10#$number))"
				;;
			m)
				printf '%s\n' "$((10#$number * 60))"
				;;
			h)
				printf '%s\n' "$((10#$number * 3600))"
				;;
			d)
				printf '%s\n' "$((10#$number * 86400))"
				;;
		esac
		return
	fi

	if [[ "$value" =~ ^[0-9]+$ ]]; then
		printf '%s\n' "$((10#$value))"
		return
	fi

	printf '%s\n' 600
}

RUST_LOG_CUTOFF_EPOCH="$(($(date +%s) - $(last_window_seconds)))"

epoch_to_utc_second() {
	local epoch="$1"

	if date -u -r "$epoch" '+%Y-%m-%dT%H:%M:%S' 2>/dev/null; then
		return
	fi
	date -u -d "@$epoch" '+%Y-%m-%dT%H:%M:%S'
}

RUST_LOG_CUTOFF_STAMP="$(epoch_to_utc_second "$RUST_LOG_CUTOFF_EPOCH")"

rust_log_modified_epoch() {
	local path="$1"

	if stat -f %m "$path" 2>/dev/null; then
		return
	fi
	if stat -c %Y "$path" 2>/dev/null; then
		return
	fi
	printf '%s\n' 0
}

rust_log_is_recent() {
	local rust_log="$1"
	local modified_epoch

	modified_epoch="$(rust_log_modified_epoch "$rust_log")"
	if [[ ! "$modified_epoch" =~ ^[0-9]+$ ]]; then
		modified_epoch=0
	fi

	[[ "$modified_epoch" -ge "$RUST_LOG_CUTOFF_EPOCH" ]]
}

filter_rust_log() {
	local rust_log="$1"
	awk -v cutoff="$RUST_LOG_CUTOFF_STAMP" '
		function has_rust_timestamp(value) {
			return length(value) >= 20 \
				&& substr(value, 5, 1) == "-" \
				&& substr(value, 8, 1) == "-" \
				&& substr(value, 11, 1) == "T" \
				&& substr(value, 14, 1) == ":" \
				&& substr(value, 17, 1) == ":"
		}
		has_rust_timestamp($1) && substr($1, 1, 19) >= cutoff {
			print
		}
	' "$rust_log"
}

copy_recent_rust_logs() {
	local destination_dir="$1"
	mkdir -p "$destination_dir"
	if [[ ! -d "$RUST_LOG_DIR" ]]; then
		return
	fi
	while IFS= read -r -d '' rust_log; do
		if ! rust_log_is_recent "$rust_log"; then
			continue
		fi
		local destination="$destination_dir/$(basename "$rust_log")"
		local filtered_destination="$destination.tmp"
		filter_rust_log "$rust_log" >"$filtered_destination"
		if [[ -s "$filtered_destination" ]]; then
			mv "$filtered_destination" "$destination"
		else
			rm -f "$filtered_destination"
		fi
	done < <(find "$RUST_LOG_DIR" -maxdepth 1 -type f -name 'rsnap*.log*' -print0)
}

print_rust_logs_in_dir() {
	local source_dir="$1"
	if [[ ! -d "$source_dir" ]]; then
		return
	fi
	while IFS= read -r -d '' rust_log; do
		cat "$rust_log"
	done < <(find "$source_dir" -maxdepth 1 -type f -name 'rsnap*.log*' -print0)
}

append_recent_rust_logs() {
	local destination="$1"
	if [[ ! -d "$RUST_LOG_DIR" ]]; then
		return
	fi
	while IFS= read -r -d '' rust_log; do
		if ! rust_log_is_recent "$rust_log"; then
			continue
		fi
		filter_rust_log "$rust_log" >>"$destination"
	done < <(find "$RUST_LOG_DIR" -maxdepth 1 -type f -name 'rsnap*.log*' -print0)
}

summarize_file() {
	local source="$1"
	awk '
		function field_value(prefix, value) {
			sub("^" prefix, "", value)
			gsub(/^"/, "", value)
			gsub(/"$/, "", value)
			return value
		}
		{
			for (i = 1; i <= NF; i++) {
				if ($i ~ /^event=/) {
					events[field_value("event=", $i)]++
				} else if ($i ~ /^metric=/) {
					metrics[field_value("metric=", $i)]++
				} else if ($i ~ /^op=/) {
					ops[field_value("op=", $i)]++
				} else if ($i ~ /^captureID=/) {
					value = field_value("captureID=", $i)
					captures[value]++
					if (value != "0") {
						latest_capture_id = value
					}
				} else if ($i ~ /^runID=/) {
					value = field_value("runID=", $i)
					runs[value]++
					latest_run_id = value
				} else if ($i ~ /^run_id=/) {
					value = field_value("run_id=", $i)
					runs[value]++
					latest_run_id = value
				}
			}
		}
		function print_group(title, values, key) {
			print title
			for (key in values) {
				print "  " key " " values[key]
			}
		}
		END {
			print "latest"
			print "  nonzero_capture_id " (latest_capture_id == "" ? "none" : latest_capture_id)
			print "  run_id " (latest_run_id == "" ? "none" : latest_run_id)
			print_group("events", events)
			print_group("metrics", metrics)
			print_group("ops", ops)
			print_group("capture_ids", captures)
			print_group("run_ids", runs)
		}
	' "$source"
}

case "$MODE" in
	show)
		/usr/bin/log show --info --style compact --last "$LAST" --predicate "$NATIVE_PREDICATE"
		;;
	stream)
		/usr/bin/log stream --info --style compact --predicate "$NATIVE_PREDICATE"
		;;
	collect)
		mkdir -p "$OUT_DIR/rust"
		write_native_log "$OUT_DIR/native-host.oslog"
		copy_recent_rust_logs "$OUT_DIR/rust"
		{
			cat "$OUT_DIR/native-host.oslog"
			print_rust_logs_in_dir "$OUT_DIR/rust"
		} >"$OUT_DIR/all.log"
		summarize_file "$OUT_DIR/all.log" >"$OUT_DIR/summary.txt"
		printf '%s\n' "$OUT_DIR"
		;;
	summary)
		tmp_file="$(mktemp)"
		trap 'rm -f "$tmp_file"' EXIT
		write_native_log "$tmp_file"
		append_recent_rust_logs "$tmp_file"
		summarize_file "$tmp_file"
		;;
	--help|-h|help)
		usage
		;;
	*)
		usage >&2
		exit 2
		;;
esac
