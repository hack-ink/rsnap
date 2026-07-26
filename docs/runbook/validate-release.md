---
title: "Validate Release"
description: "Validate Release documentation for Rsnap."
type: "Runbook"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-26
---
# Validate Release

Goal: Execute the final release-candidate checks before publishing a tagged Rsnap release.

Read this when: You are preparing a formal Rsnap release tag or verifying the published release
artifacts immediately after the tag workflow completes.

Preconditions: `main` is clean and synced. The intended release version is committed in
`Cargo.toml` and `Cargo.lock`. The repository has a protected GitHub environment named `release`.
All required signing, Team API notary, and Rsnap Sparkle values are GitHub Actions organization
secrets in `acg-box`, with `selected` repository access granted to `acg-box/rsnap`. The Release
workflow uses the standard GitHub-hosted `macos-26` ARM64 runner. A logged-in macOS desktop session
is available for native-host smoke and manual checks.

Depends on: `docs/spec/release-distribution.md`; `docs/spec/app-identity.md`;
`docs/spec/settings.md`; `docs/spec/telemetry.md`; `docs/runbook/performance-validation.md`;
`.github/workflows/release.yml`

Verification: Local checks, dedicated macOS smoke/perf evidence, source-provenance validation,
Developer ID signing evidence, accepted notarization, a valid staple, Gatekeeper acceptance,
Sparkle and checksum validation, draft-asset validation, and manual first-run/user-flow validation.

## Before Tagging

1. Confirm the release version:
   - `Cargo.toml` `workspace.package.version` matches the intended tag without a leading `v`.
   - Rsnap workspace package versions in `Cargo.lock` match the intended version.
   - `Package.swift` and `Package.resolved` contain the same exact Sparkle version.
   - No existing local or remote tag already uses `v<version>`.
   - The future annotated tag commit is present on `origin/main`.
2. Confirm the `release` environment and organization secret binding:
   - The protected `release` environment exists and has the required deployment protection rules.
   - The environment is the deployment protection boundary. It does not store the long-lived release
     secrets.
   - Each required secret is an `acg-box` organization Actions secret with visibility `selected`
     and repository access granted to `acg-box/rsnap`:
     - `APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64`
     - `APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD`
     - `APPLE_DEVELOPER_ID_APPLICATION_IDENTITY`
     - `APPLE_NOTARY_KEY_ID`
     - `APPLE_NOTARY_ISSUER_ID`
     - `APPLE_NOTARY_KEY_P8`
     - `RSNAP_SPARKLE_PRIVATE_ED_KEY`
   - No repository or environment secret has one of these names. A narrower-scope value would
     override the organization secret.
   - The Apple identity is an exact Developer ID Application identity.
   - The notary key is a Team API key. Its issuer ID is mandatory.
   - The Rsnap Sparkle private key derives the `SUPublicEDKey` in `scripts/build_and_run.sh`.
     Do not use a generic Sparkle secret, grant this secret to another application repository, or
     use a private key for another application.
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
- Scroll Capture must stay absent for window-click and fullscreen freezes, remain available after
  dragged-region movement or auto-center, start from a dragged-region freeze through the toolbar
  or plain `s`, and pass the functional scroll path in
  `docs/runbook/scroll-capture-recovery-plan.md`. Scroll toolbar Liquid Glass cadence, dynamic
  backdrop-change evidence, preview export latency, and cached copy/export timing are part of the
  current Scroll Capture publish gate.
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
2. Confirm that the source-validation job accepts the exact tag, Cargo and Sparkle versions, peeled
   commit, checkout, and `origin/main` ancestry.
3. Confirm that the `macos-26` job completes release tests, release build, inside-out signing,
   accepted notarization, staple validation, recursive signature verification, and Gatekeeper
   assessment. Record the notarization submission UUID from the log. If the wait times out, use
   that UUID to inspect the Apple submission; do not publish or staple the package.
4. Confirm that the Ubuntu job creates a draft, uploads exactly the ZIP, appcast, and checksum,
   rechecks the remote annotated tag commit immediately before publication, validates the Ed25519
   signature, remote metadata, and downloaded bytes, and only then makes the release public. Draft
   API URLs can contain one temporary `untagged-*` slug, but appcast URLs must contain the canonical
   `acg-box/rsnap` repository and final tag.
5. Treat any test, build, signing, timestamp, notarization, staple, Gatekeeper, appcast, checksum,
   upload, or draft-validation failure as a release blocker. Do not publish a package from a failed
   run.
6. The workflow does not publish crates.io packages or non-macOS desktop archives.
7. If publication succeeded but the final API response was lost, rerun the workflow. The publish
   script validates the existing public metadata and downloaded public bytes without changing the
   release. It does not compare the public assets with the new nondeterministic signed build.

## Published Artifact Check

After the Release workflow succeeds:

1. Download `rsnap-aarch64-apple-darwin.zip` from the GitHub release or from:
   `https://github.com/acg-box/rsnap/releases/latest/download/rsnap-aarch64-apple-darwin.zip`
   Also download
   `https://github.com/acg-box/rsnap/releases/latest/download/rsnap-aarch64-apple-darwin.zip.sha256`.
2. Verify the checksum before extraction:

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
4. Verify the signature and staple:

```sh
codesign --verify --deep --strict /path/to/Rsnap.app
xcrun stapler validate -v /path/to/Rsnap.app
```

5. Verify Gatekeeper acceptance:

```sh
spctl -a -vvv --type exec /path/to/Rsnap.app
```

6. Confirm the appcast asset was published:

```sh
curl -fsSL https://github.com/acg-box/rsnap/releases/latest/download/appcast.xml \
  | grep -q 'sparkle:edSignature'
```

7. Launch the downloaded app and repeat a minimal capture, toolbar, OCR, copy, save, and About
   update check.
8. Confirm release notes, the macOS ZIP, appcast, and checksum were published.
