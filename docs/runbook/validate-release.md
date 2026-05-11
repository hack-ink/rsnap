# Validate Release

Goal: Execute the final release-candidate checks before publishing a tagged Rsnap release.

Read this when: You are preparing a formal Rsnap release tag or verifying the published release
artifacts immediately after the tag workflow completes.

Preconditions: `main` is clean and synced, the intended release version is committed in
`Cargo.toml`, GitHub Actions macOS signing release secrets are configured, optional Apple notary
credentials are configured only when a notarized build is required, the Release workflow runs the
macOS package job on `macos-26` with Apple Swift 6.2 or newer so Liquid Glass API support is
compiled into the app, and a logged-in macOS desktop session is available for native-host smoke and
manual checks.

Depends on: `docs/spec/app-identity.md`; `docs/spec/settings.md`; `docs/spec/telemetry.md`;
`docs/runbook/performance-validation.md`; `.github/workflows/release.yml`

Verification: Local checks, dedicated macOS smoke/perf evidence, release workflow success, signed
macOS zip acceptance, optional notarization evidence when notary credentials are configured, and
manual first-run/user-flow validation.

## Before Tagging

1. Confirm the release version:
   - `Cargo.toml` `workspace.package.version` matches the intended tag without a leading `v`.
   - No existing local or remote tag already uses `v<version>`.
2. Confirm release credentials:
   - Apple signing certificate secrets are available to the Release workflow.
   - Sparkle update signing is configured: `SUPublicEDKey` is checked into
     `scripts/build_and_run.sh`, and `SPARKLE_PRIVATE_ED_KEY` is available to the Release workflow
     for signing the published update archive.
   - Apple notary credentials are optional for current preview releases; when absent, the Release workflow still
     publishes a signed but unnotarized macOS zip.
3. Confirm local gates:
   - `cargo make checks`
   - `cargo make test-host-reset`
   - `cargo make test-macos-native-host-stage`
4. Confirm dedicated desktop validation:
   - `scripts/smoke/macos.sh`
   - `scripts/perf/macos.sh`
   - If scroll-capture correctness changed, follow the recorded-trace flow in
     `docs/runbook/performance-validation.md`.

## Manual RC Smoke

Launch the staged signed app path:

```sh
RSNAP_NATIVE_HOST_FORCE_REBUILD=1 ./scripts/build_and_run.sh run
```

Validate these user-visible flows:

- First launch and missing Screen Recording recovery.
- Menubar app identity, no Dock icon during capture, Settings open/close behavior.
- Option-X capture start, cancel, and frontmost-app restoration.
- Live HUD, Tab loupe sampling, window outline, dragged-region freeze, click-window freeze, and
  fullscreen fallback.
- Frozen toolbar tools: pointer, pen, arrow, text, mosaic, spotlight, undo, redo, auto-center,
  Recognize Text, copy, and save.
- Scroll capture is hidden in the v0.2.1 native-host release: the toolbar must not show a scroll
  capture item, and pressing `s` must not enter scroll capture.
- Light and dark appearance; Classic Glass and Liquid Glass where the OS and current build support
  Liquid Glass.
- Settings -> About update rows: `Auto Update` and `Release Version` must use Title Case for row
  titles. The Auto Update mode control must show `Off`, `Notify`, and `Install`; secondary text must use
  sentence case, must not look like download progress, and the release-configured build must not
  report that the Sparkle appcast is missing.
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
2. Watch the Release workflow for the exact tag.
3. Treat a build, signing, or packaging failure as a release blocker.
4. Treat notarization failure as a release blocker only when notary credentials are configured.
5. The Release workflow publishes the signed macOS zip and `appcast.xml` to the GitHub release.
   It notarizes and staples the app only when notary credentials are configured. It does not
   publish crates.io packages or non-macOS desktop archives for current preview releases.

## Published Artifact Check

After the Release workflow succeeds:

1. Download `rsnap-aarch64-apple-darwin.zip` from the GitHub release or from:
   `https://github.com/hack-ink/rsnap/releases/latest/download/rsnap-aarch64-apple-darwin.zip`
2. Unzip it and verify identity:
   - The app bundle is `Rsnap.app`.
   - `CFBundleName` and `CFBundleDisplayName` are `Rsnap`.
   - `CFBundleIdentifier` is `ink.hack.rsnap`.
   - `SUFeedURL` is
     `https://github.com/hack-ink/rsnap/releases/latest/download/appcast.xml`.
   - `SUPublicEDKey` is present.
   - `Sparkle.framework` is present in `Contents/Frameworks`.
3. Verify the signature:

```sh
codesign --verify --deep --strict /path/to/Rsnap.app
```

4. For a notarized build, verify Gatekeeper acceptance:

```sh
spctl -a -vvv --type exec /path/to/Rsnap.app
```

For a signed but unnotarized build, Gatekeeper may still block a quarantined download. Use the
quarantine override documented in `README.md` only for a bundle built locally or downloaded from
this repository's GitHub Releases page.

5. Confirm the appcast asset was published:

```sh
curl -fsSL https://github.com/hack-ink/rsnap/releases/latest/download/appcast.xml \
  | grep -q 'sparkle:edSignature'
```

6. Launch the downloaded app and repeat a minimal capture, toolbar, OCR, copy, save, and About
   update check.
7. Confirm release notes, the macOS zip, and the appcast were published.
