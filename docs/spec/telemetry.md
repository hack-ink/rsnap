# Telemetry Schema

Purpose: Define the required telemetry fields, source boundaries, and naming rules for
rsnap runtime debugging.

Status: normative

Read this when: Changing logs, adding timing metrics, writing diagnostic scripts, or
debugging a native-host capture path.

Not this document: Use `docs/runbook/telemetry-debugging.md` for the collection sequence.

Defines: Native-host OSLog fields, Rust tracing fields, category ownership, event names,
metric names, and correlation identifiers.

## Sources

rsnap has two active telemetry sources.

| Source | Transport | Schema | Primary file |
| --- | --- | --- | --- |
| macOS native host | Unified Logging / OSLog | `rsnap.native_host.telemetry/1` | `native/macos-host/Sources/RsnapNativeHostKit/NativeHostTelemetry.swift` |
| Rust desktop/runtime | `tracing` rolling file logs | `rsnap.rust.telemetry/1` | `apps/rsnap/src/startup.rs` |

Native-host events are the authoritative source for screenshot startup, live sampling,
Frozen transition, pasteboard write, and native ScreenCaptureKit lifecycle timing.

Rust tracing events are the authoritative source for the older Rust overlay runtime,
scroll capture, live frame stream internals, and file-log initialization.

## Required Fields

Every native-host telemetry line must include:

- `schema=rsnap.native_host.telemetry/1`
- `runID=<uuid>`
- `event=<stable.name>` or `metric=<stable.name>`

Every native-host capture-session event must include:

- `captureID=<integer>`
- `captureID=0` only for process-level startup or prewarm work outside an active capture.

Every Rust process telemetry line that identifies the process run must include:

- `schema=rsnap.rust.telemetry/1`
- `run_id=<pid-started_at_ms>`
- `op=<stable.name>`

Every new Rust tracing event must include:

- `op=<stable.name>`
- Structured fields for dynamic values such as ids, counts, durations, paths, and errors.

## Native-Host Categories

| Category | Owns |
| --- | --- |
| `Lifecycle` | App startup, status item setup, sound loading, process prewarm status |
| `Capture` | Non-timing capture lifecycle events and failures |
| `CaptureTiming` | Capture-session phase timings and capture output timings |
| `FrozenFrameAuthority` | ScreenCaptureKit frozen-frame stream setup, first frame, and stream failures |
| `LiveChromeTelemetry` | Live HUD/loupe refresh target and batched live chrome metrics |

Do not reintroduce raw `NSLog` for native-host runtime diagnostics. Route new native-host
diagnostics through `NativeHostTelemetry`.

Collection scripts must default to schema-qualified native-host OSLog entries:

```text
subsystem == "ink.hack.rsnap" AND composedMessage CONTAINS "schema=rsnap.native_host.telemetry/1"
```

Use an explicit predicate override only when investigating legacy or non-telemetry OSLog
noise.

Rust collection helpers must filter rolling log entries to the selected collection window
using the leading UTC timestamp emitted by `tracing_subscriber::fmt`.

## Event And Metric Names

Use dotted lowercase names:

- `native_host.finish_launching_begin`
- `capture_timing.start_capture`
- `capture_timing.freeze_commit`
- `capture_timing.copy_capture`
- `frozen_authority.stream_start_failed`
- `live_chrome.refresh_target`
- `logging.file_initialized`

Event names describe one observation. Metric names describe one sampled distribution.
Do not encode values into the event name; emit values as fields.

## Units

- Durations use milliseconds and end in `Ms`.
- Rates use hertz and end in `Hz`.
- Boolean values use `true` or `false`.
- Pixel dimensions use `width` and `height`.
- OS display identifiers use `displayID`.
- Frozen capture frame provenance uses `snapshotSource`; `post_token` means ScreenCaptureKit
  produced a frame after the frozen latch, `latest_unchanged` means no newer frame arrived
  and the current stream's latest same-sequence frame was used for an unchanged/static desktop,
  `authority_latest` means the frozen-authority stream's already-warm frame was used immediately
  for the release handoff, `live_sampler_latest` means the already-warm live sampler supplied the
  release-time monitor frame. The release handoff must not use a synchronous full-monitor
  `CGWindowList` capture as a first-frame fallback because that path can add visible latency and
  can differ subtly from the live ScreenCaptureKit frame in window shadow/framing treatment.
- Rust identifiers use snake_case unless they are mirroring existing platform names.

## Capture Correlation

`CaptureSessionController` owns `captureID` allocation. A single user-visible capture
session keeps the same `captureID` from startup through teardown.

Expected capture-session chain:

1. `capture_timing.live_sampling_warm`
2. `capture_timing.start_capture`
3. `capture_timing.freeze_commit` or `capture_timing.freeze_commit_failed`
4. `capture_timing.frozen_selection_image`
5. `capture_timing.copy_capture`
6. `capture.teardown`

Missing events in the chain are evidence of the next failing phase and should be treated as
debug signal, not as logging noise.

## Live Chrome Correlation

`live_chrome.refresh_target` must include:

- `captureID`
- `targetHz`
- `frameBudgetMs`
- `hudGlassEnabled`
- `hudGlassMode`
- `liquidGlassStyle`
- `liquidGlassAvailable`

The live chrome refresh-target dedupe key must include the glass fields above, because the
rendering path can change without `targetHz` changing.

## Privacy

Native-host telemetry can mark diagnostic fields public when they are needed for debugging.
Do not log captured image contents, OCR text, clipboard contents, or user-entered annotation
text. Paths may be logged only when they identify app resources or log artifacts.
