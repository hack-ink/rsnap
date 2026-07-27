---
title: "Validate Release"
description: "Validate Release documentation for Rsnap."
type: "Runbook"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-27
---
# Validate Release

Goal: Execute the final release-candidate checks before publishing a tagged Rsnap release.

Read this when: You are preparing a formal Rsnap release tag or verifying the published release
artifacts immediately after the tag workflow completes.

Preconditions: `main` is clean and synced. The intended release version is committed in
`Cargo.toml` and `Cargo.lock`. The four required signing and update values are `acg-box`
organization Actions secrets with visibility `all`. The `release` environment is configured for
`v*` tags and protects the publish job; it does not store release secrets. The Release workflow
runs the macOS package job on the standard GitHub-hosted `macos-26` ARM64 runner with Apple Swift
6.2 or newer so Liquid Glass API support is compiled into the app. A logged-in macOS desktop
session is available for native-host smoke and manual checks.

Depends on: `docs/spec/release-distribution.md`; `docs/spec/app-identity.md`;
`docs/spec/settings.md`; `docs/spec/telemetry.md`; `docs/runbook/performance-validation.md`;
`.github/workflows/release.yml`

Verification: Local checks, dedicated macOS smoke/perf evidence, source-provenance validation,
release workflow success, signed macOS ZIP and Sparkle appcast validation, SHA-256 verification,
draft-asset validation, and manual first-run/user-flow validation.

## Before Tagging

1. Confirm the release version:
   - `Cargo.toml` `workspace.package.version` matches the intended tag without a leading `v`.
   - Rsnap workspace package versions in `Cargo.lock` match the intended version.
   - `Package.swift` and `Package.resolved` contain the same exact Sparkle version.
   - No existing local or remote tag already uses `v<version>`.
   - Create an annotated `vX.Y.Z` tag only from a commit that is on `origin/main`. The workflow
     rejects lightweight tags, mismatched checkouts, and commits that are not reachable from
     `origin/main`.
   - Repository tag rulesets restrict `v*` creation to the authorized operator and prevent updates
     or deletion after creation.
2. Confirm release credentials:
   - These required organization Actions secrets are available to the Release workflow:
     - `APPLE_CERTIFICATE_P12_BASE64`
     - `APPLE_CERTIFICATE_PASSWORD`
     - `APPLE_SIGNING_IDENTITY`
     - `SPARKLE_PRIVATE_ED_KEY`
   - Sparkle update signing is configured: `SUPublicEDKey` is checked into
     `scripts/build_and_run.sh`, and `SPARKLE_PRIVATE_ED_KEY` is available to the Release workflow
     for signing the published update archive.
   - The workflow signs the known Rsnap and Sparkle code graph from the inside out with the current
     Apple Development identity and Hardened Runtime. It does not use `codesign --deep` to sign.
   - The current Personal Team package is signed with Apple Development and is not notarized.
     Users can need the Gatekeeper override documented in `README.md`.
   - The `release` environment contains no secrets and does not replace the organization secret
     contract. Its reviewer and `v*` deployment policy gate only the final publish job.
3. Confirm local gates:
   - `cargo make checks`
   - `cargo make test-host-reset`
   - `cargo make test-macos-native-host-stage`
   - `cargo make test-release`
   - `actionlint .github/workflows/*.yml`
4. Confirm dedicated desktop validation:
   - `scripts/smoke/macos.sh`
   - `scripts/perf/macos.sh`
   - If scroll-capture correctness changed, follow the deterministic test, perf, and native-smoke
     sequence in `docs/runbook/performance-validation.md`.

## Manual RC Smoke

Launch the staged signed app path:

```sh
RSNAP_NATIVE_HOST_FORCE_REBUILD=1 ./scripts/build_and_run.sh run
```

Validate these user-visible flows:

- First launch and missing Screen Recording recovery.
- Menubar app identity, no Dock icon during capture, Settings open/close behavior.
- Option-X capture start, cancel, and frontmost-app restoration.
- Option-Shift-X quick screenshot start, transient-menu preservation, dragged-region freeze,
  cancel, and frontmost-app restoration.
- Status menu exposes Quick Screenshot and shows the configured quick screenshot shortcut. Settings
  -> Capture shows canonical `Option-X` and `Option-Shift-X` shortcut values; changing the Quick
  Screenshot shortcut persists, updates the status-menu item, and still starts quick screenshot
  without activating Rsnap. If event-tap setup fails, collect native-host telemetry and treat it as
  a quick screenshot blocker.
- Live HUD, Tab loupe sampling, window outline, dragged-region freeze, click-window freeze, and
  fullscreen fallback.
- Frozen toolbar tools: pointer, pen, arrow, text, mosaic, spotlight, undo, redo, auto-center,
  Recognize Text, Scroll Capture, copy, and save.
- Scroll Capture must stay absent for window-click and
  fullscreen freezes, remain available after dragged-region movement or auto-center, start from a
  dragged-region freeze via toolbar or plain `s`, and pass the functional scroll path in
  `docs/runbook/scroll-capture-recovery-plan.md`. Scroll toolbar Liquid Glass cadence, dynamic
  backdrop-change evidence, preview export latency, and cached copy/export timing are part of the
  publish gate.
- Light and dark appearance; Classic Glass and Liquid Glass where the OS and current build support
  Liquid Glass.
- Settings -> About update rows: `Auto Update` and `Release Version` must use Title Case for row
  titles. The Auto Update mode control must show `Off`, `Notify`, and `Install`; secondary text must use
  sentence case, must not look like download progress, and the release-configured build must not
  report that the Sparkle appcast is missing. The `Check` button must stay enabled after starting
  an update from the status menu or while Sparkle has an active update session.
- Settings -> Output frame rows: `Frame Preset` must include `Off`, selecting `Off` must disable
  `Apply To`, and selecting a background preset must re-enable `Apply To`.
- Sparkle local update smoke:

```sh
scripts/smoke/sparkle-update-local.sh
```

  The script builds a disposable old app, a higher-version update archive, a local signed appcast,
  and a local HTTP server. The final Sparkle `Install and Relaunch` confirmation remains manual;
  after confirming it, return to the script and press Enter so it can verify the old bundle's
  `CFBundleVersion` changed to the update version.
- Output directory, filename prefix, sequence/timestamp naming, clipboard copy, and save failure
  handling where practical.

Collect telemetry after any surprising behavior:

```sh
scripts/telemetry/native-host.sh collect
```

Telemetry artifacts must not include captured image contents, OCR text, clipboard contents, or
user-entered annotation text.

## Tag And Publish

1. Push the annotated release tag only after local and manual RC validation pass.
2. Confirm that the source-validation job accepts the exact stable SemVer tag, Cargo and Sparkle
   versions, annotated tag object, checked-out commit, event commit, and `origin/main` ancestry.
3. Confirm that the single `macos-26` job completes release tests, assembly, inside-out signing,
   recursive signature verification, final ZIP creation, Sparkle appcast generation, and checksum
   validation.
4. Confirm that the workflow records the expected signed-but-unnotarized Personal Team warning.
5. Approve the `release` environment only after the macOS job succeeds. The Ubuntu publisher
   creates or reuses a draft, uploads exactly the ZIP, appcast, and checksum, validates remote
   metadata and downloaded bytes, and only then makes the release public. Any pre-publication
   validation or upload failure leaves the release as a draft.
6. The workflow does not publish crates.io packages or non-macOS desktop archives.

## Published Artifact Check

After the Release workflow succeeds:

1. Download `rsnap-aarch64-apple-darwin.zip` from the GitHub release or from:
   `https://github.com/acg-box/rsnap/releases/latest/download/rsnap-aarch64-apple-darwin.zip`
   Also download
   `https://github.com/acg-box/rsnap/releases/latest/download/rsnap-aarch64-apple-darwin.zip.sha256`.
2. Verify the checksum:

```sh
shasum -a 256 -c rsnap-aarch64-apple-darwin.zip.sha256
```

3. Unzip it and verify identity:
   - The app bundle is `Rsnap.app`.
   - `CFBundleName` and `CFBundleDisplayName` are `Rsnap`.
   - `CFBundleIdentifier` is `ink.hack.rsnap`.
   - `SUFeedURL` is
     `https://github.com/acg-box/rsnap/releases/latest/download/appcast.xml`.
   - `SUPublicEDKey` is present.
   - `Sparkle.framework` is present in `Contents/Frameworks`.
4. Verify the signature:

```sh
codesign --verify --deep --strict /path/to/Rsnap.app
```

5. Gatekeeper can block the signed but unnotarized quarantined download. Use the
quarantine override documented in `README.md` only for a bundle built locally or downloaded from
this repository's GitHub Releases page.

6. Confirm the appcast asset was published:

```sh
curl -fsSL https://github.com/acg-box/rsnap/releases/latest/download/appcast.xml \
  | grep -q 'sparkle:edSignature'
```

7. Launch the downloaded app and repeat a minimal capture, toolbar, OCR, copy, save, and About
   update check.
8. Confirm release notes, the macOS ZIP, appcast, and checksum were published.
