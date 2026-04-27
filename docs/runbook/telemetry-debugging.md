# Telemetry Debugging

Goal: Collect and summarize rsnap native-host and Rust telemetry for screenshot,
ScreenCaptureKit, live chrome, and capture-output debugging.

Read this when: A local run has a performance, startup, frozen-frame, pasteboard, live
chrome, or scroll-capture symptom that needs evidence.

Inputs: A local checkout, macOS Unified Logging, and the native host launched through
`scripts/build_and_run.sh`.

Depends on: `docs/spec/telemetry.md` for the field contract.

Outputs: A telemetry artifact directory containing native OSLog output, Rust rolling logs,
combined logs, and a compact summary.

## Launch The Current Build

Use the signed release path when testing user-visible native-host behavior:

```sh
RSNAP_NATIVE_HOST_FORCE_REBUILD=1 ./scripts/build_and_run.sh run
```

Use the streaming mode when you need live logs while reproducing:

```sh
./scripts/build_and_run.sh --telemetry
```

`--telemetry` streams through `scripts/telemetry/native-host.sh`, so it uses the same
predicate as the collection path below.

## Collect An Artifact

Run:

```sh
scripts/telemetry/native-host.sh collect
```

The command prints the artifact directory. By default it writes under:

```text
target/telemetry/native-host-YYYYMMDD-HHMMSS/
```

The artifact contains:

- `native-host.oslog`: macOS Unified Logging entries for `ink.hack.rsnap` native-host telemetry
- `rust/`: filtered Rust rolling log files containing entries inside the selected telemetry window
- `all.log`: native-host and Rust logs concatenated for quick grep
- `summary.txt`: latest run/capture identifiers plus counts grouped by `event`, `metric`,
  `op`, `captureID`, and run id

Use a custom output directory when comparing multiple runs:

```sh
RSNAP_TELEMETRY_OUT_DIR=target/telemetry/cold-capture-a scripts/telemetry/native-host.sh collect
```

Use a custom time window when the symptom happened earlier:

```sh
RSNAP_TELEMETRY_LAST=30m scripts/telemetry/native-host.sh collect
```

`RSNAP_TELEMETRY_LAST` applies to macOS Unified Logging through `log show --last`.
For Rust rolling logs, the same value first skips stale rolling files by modification time
and then filters entries by their leading UTC timestamp, so old process logs do not
pollute a fresh artifact. The helper supports `s`, `m`, `h`, and `d` suffixes for
Rust-log filtering; widen the window or set `RSNAP_RUST_LOG_DIR` when you intentionally
need older Rust logs.

## Show Or Summarize Without Creating An Artifact

Show recent native-host telemetry:

```sh
RSNAP_TELEMETRY_LAST=5m scripts/telemetry/native-host.sh show
```

Summarize native-host telemetry plus available Rust logs:

```sh
RSNAP_TELEMETRY_LAST=5m scripts/telemetry/native-host.sh summary
```

The summary command uses the same native-host predicate and Rust-log modification window
as `collect`.

## Interpret Capture Sessions

Start with `summary.txt` and read `latest.nonzero_capture_id`.

Then inspect the chain:

```sh
grep 'captureID=1' target/telemetry/native-host-*/all.log
```

Expected capture chain:

1. `capture_timing.live_sampling_warm`
2. `capture_timing.start_capture`
3. `capture_timing.freeze_commit` or `capture_timing.freeze_commit_failed`
4. `capture_timing.frozen_selection_image`
5. `capture_timing.copy_capture`
6. `capture.teardown`

If the chain stops before `capture_timing.start_capture`, focus on permissions,
warm sampling, window snapshots, or overlay show timing.

If the chain stops at `capture_timing.freeze_commit_failed`, inspect
`FrozenFrameAuthority` entries for ScreenCaptureKit content lookup, stream start, or first
frame timing.

If `capture_timing.copy_capture` is slow, compare `captureImageMs`, `makeImageMs`, and
`writePasteboardMs` before changing capture code.

## Interpret Live Chrome

Find:

```sh
grep 'event=live_chrome.refresh_target' target/telemetry/native-host-*/all.log
```

The entry records the cadence and rendering path:

- `captureID`
- `targetHz`
- `frameBudgetMs`
- `hudGlassEnabled`
- `hudGlassMode`
- `liquidGlassStyle`
- `liquidGlassAvailable`

When visual performance differs across settings, compare these fields before comparing
duration metrics.

## Interpret Rust Runtime Logs

Rust logs aggregate by `op`.

Use:

```sh
grep 'op=' target/telemetry/native-host-*/all.log
```

Common useful prefixes:

- `overlay.*`
- `live_frame_stream.*`
- `scroll_capture.*`
- `scroll_input.*`
- `logging.file_initialized`
- `rsnap.starting`

Rust startup entries include `schema=rsnap.rust.telemetry/1` and `run_id=...`.

## Required Closeout Evidence

For a telemetry-sensitive fix, report:

- The launched native-host PID.
- The artifact directory or exact `log show` query.
- The relevant `runID`.
- The relevant `captureID`, when a capture session was exercised.
- The commands run from `Makefile.toml`, such as `cargo make lint-swift` or
  `cargo make lint-rust`.
