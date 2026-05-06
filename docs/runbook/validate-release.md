# Validate Release

Goal: Execute the final release-candidate checks before publishing a tagged Rsnap release.

Read this when: You are preparing a formal Rsnap release tag or verifying the published release
artifacts immediately after the tag workflow completes.

Preconditions: `main` is clean and synced, the intended release version is committed in
`Cargo.toml`, GitHub Actions macOS signing/notary release secrets are configured, and a logged-in
macOS desktop session is available for native-host smoke and manual checks.

Depends on: `docs/spec/app-identity.md`; `docs/spec/settings.md`; `docs/spec/telemetry.md`;
`docs/runbook/performance-validation.md`; `.github/workflows/release.yml`

Verification: Local checks, dedicated macOS smoke/perf evidence, release workflow success, signed
and notarized macOS zip acceptance, and manual first-run/user-flow validation.

## Before Tagging

1. Confirm the release version:
   - `Cargo.toml` `workspace.package.version` matches the intended tag without a leading `v`.
   - No existing local or remote tag already uses `v<version>`.
2. Confirm release credentials:
   - Apple signing certificate and notary credentials are available to the Release workflow.
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
- Scroll capture is hidden in the v0.1.0 native-host release: the toolbar must not show a scroll
  capture item, and pressing `s` must not enter scroll capture.
- Light and dark appearance; Classic Glass and Liquid Glass where the OS supports Liquid Glass.
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
3. Treat a build, signing, notarization, or packaging failure as a release blocker.
4. The Release workflow publishes the notarized macOS zip plus checksum files to the GitHub
   release. It does not publish crates.io packages or non-macOS desktop archives for v0.1.0.

## Published Artifact Check

After the Release workflow succeeds:

1. Download `rsnap-aarch64-apple-darwin.zip` from the GitHub release or from:
   `https://github.com/hack-ink/rsnap/releases/latest/download/rsnap-aarch64-apple-darwin.zip`
2. Unzip it and verify identity:
   - The app bundle is `Rsnap.app`.
   - `CFBundleName` and `CFBundleDisplayName` are `Rsnap`.
   - `CFBundleIdentifier` is `ink.hack.rsnap`.
3. Verify signature and Gatekeeper acceptance:

```sh
codesign --verify --deep --strict /path/to/Rsnap.app
spctl -a -vvv --type exec /path/to/Rsnap.app
```

4. Launch the downloaded app and repeat a minimal capture, toolbar, OCR, copy, and save check.
5. Confirm release notes and checksums were published with the artifacts.
