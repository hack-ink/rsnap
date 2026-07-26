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
secrets in the single-operator `acg-box` organization, with visibility `all`. The personal Infisical
release project contains the operator source record for the same values. The Release workflow uses
the standard GitHub-hosted `macos-26` ARM64 runner. A logged-in macOS desktop session is available
for native-host smoke and manual checks.

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
   - The future annotated tag commit equals the current `origin/main` tip. An older ancestor is not
     accepted.
   - GitHub ruleset `release-tags-authorized-creation` is active for `refs/tags/v*` and gives only
     user `acgxv` an `always` bypass for tag creation.
   - GitHub ruleset `release-tags-immutable` is active for `refs/tags/v*`, has no bypass, and
     prevents both update and deletion.
2. Confirm the `release` environment and organization secret binding:
   - The protected `release` environment normally requires reviewer `acgxv`, permits self-review,
     and has one deployment branch/tag policy with name `v*` and type `tag`.
   - Repository administrators can force a deployment past the environment protection rules. Treat
     that single-operator recovery action as an audited exception. It does not skip the workflow's
     source or artifact validators.
   - The environment is the deployment protection boundary. It does not store the long-lived release
     secrets.
   - Each release secret is an `acg-box` organization Actions secret with visibility `all`:
     - `APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64`
     - `APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD`
     - `APPLE_DEVELOPER_ID_APPLICATION_IDENTITY`
     - `APPLE_NOTARY_KEY_ID`
     - `APPLE_NOTARY_ISSUER_ID`
     - `APPLE_NOTARY_KEY_P8`
     - `RSNAP_SPARKLE_PRIVATE_ED_KEY`
   - No repository or environment secret has one of the organization-secret names.
   - `SPARKLE_PUBLIC_ED_KEY` is not required because the public key is checked in.
   - The Apple identity is an exact Developer ID Application identity.
   - `APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64` is the P12 file encoded as one RFC 4648 base64
     value without whitespace. Its password is
     `APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD`.
   - `APPLE_DEVELOPER_ID_APPLICATION_IDENTITY` is the exact Keychain identity text in the form
     `Developer ID Application: <name> (<team-id>)`.
   - The notary key is a Team App Store Connect API key, not an individual key. Its 10-character
     uppercase alphanumeric key ID, issuer UUID, and complete P8 text are
     `APPLE_NOTARY_KEY_ID`, `APPLE_NOTARY_ISSUER_ID`, and `APPLE_NOTARY_KEY_P8`.
   - The Rsnap Sparkle private key derives the `SUPublicEDKey` in `scripts/build_and_run.sh`. Its
     organization-wide visibility does not make it reusable by another application. Use a separate,
     application-specific secret name and value for each application.
   - Visibility `all` lets every current and future `acg-box` repository workflow request this key.
     The prefix is not access control. Review new repositories and workflow changes before they can
     enter the organization credential boundary.
   - After this workflow lands on `main`, delete the legacy repository secret
     `SPARKLE_PRIVATE_ED_KEY`. Do not promote that generic name to organization scope because a
     different application's workflow could consume the Rsnap key.
3. Confirm the personal Infisical source record:
   - `docs/spec/release-secret-topology.json` is valid version 2 desired state.
   - Repository root `.infisical.json` pins domain `http://127.0.0.1:51890`, project
     `f55a1068-0ae7-4dee-a0c0-62bfe71016fc`, and environment `prod`.
   - Machine identity `rsnap-release-provisioner` can read the Rsnap project and cannot read a
     sibling project.
   - `/release/apple` contains the six Apple values after they are provisioned.
   - `/release/sparkle` contains `RSNAP_SPARKLE_PRIVATE_ED_KEY`.
   - The Sparkle value from Infisical and the organization secret both derive the checked-in public
     key. Never print or export either value during this check.
   - GitHub-hosted runners use the GitHub organization copy. They do not connect to the loopback
     Infisical instance.
   - `docs/evidence/release-secret-topology-2026-07-26.md` contains the current value-free provider
     and consumer evidence.
4. Confirm local gates:
   - `cargo make checks`
   - `cargo make test-host-reset`
   - `cargo make test-macos-native-host-stage`
   - `cargo make test-release`
   - `actionlint .github/workflows/*.yml`
5. Confirm dedicated desktop validation:
   - `scripts/smoke/macos.sh`
   - `scripts/perf/macos.sh`
   - If scroll-capture correctness changed, follow the deterministic test, perf, and native-smoke
     sequence in `docs/runbook/performance-validation.md`.

## After Landing Secret Cleanup

Complete this sequence after the workflow lands on `main` and before the first formal release tag:

1. Confirm that `.github/workflows/release.yml` on `main` requests
   `secrets.RSNAP_SPARKLE_PRIVATE_ED_KEY` and does not request
   `secrets.SPARKLE_PRIVATE_ED_KEY`.
2. Confirm that the `acg-box` organization secret `RSNAP_SPARKLE_PRIVATE_ED_KEY` has visibility
   `all`, and that no repository or `release` environment secret shadows that exact name.
3. Confirm the value-free canary result in
   `docs/evidence/release-secret-topology-2026-07-26.md`.
4. Delete only the exact compatibility repository secret:

```sh
gh secret delete SPARKLE_PRIVATE_ED_KEY --repo acg-box/rsnap
```

5. List the organization, repository, and environment secret metadata again. Confirm that the
   generic repository secret is absent and update the value-free evidence verdict. Do not create a
   generic organization alias.
6. Keep the GitHub redirect from `hack-ink/rsnap` to `acg-box/rsnap` intact until the installed
   `v0.3.0` population has upgraded. Do not create a repository or fork at the old location.

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
   commit, checkout, and exact `origin/main` tip.
3. Confirm that the credential-free `macos-26` job completes release tests, the release build,
   bundle validation, and the immutable unsigned handoff without a release environment or release
   secret.
4. Confirm that the fresh `macos-26` signing job downloads the handoff by artifact ID, verifies its
   digest and metadata, performs inside-out signing, receives accepted notarization, staples the
   app, and passes recursive signature verification and Gatekeeper assessment. Record the
   notarization submission UUID from the log. If the wait times out, use that UUID to inspect the
   Apple submission; do not publish or staple the package.
5. Confirm that the Ubuntu job proves the target version is newer than the highest published stable
   version, creates a draft, uploads exactly the ZIP, appcast, and checksum,
   rechecks the remote annotated tag commit immediately before publication, validates the Ed25519
   signature, remote metadata, downloaded bytes, release order, and latest pointer, and only then
   makes the release public. Draft API URLs can contain one temporary `untagged-*` slug, but appcast
   URLs must contain the canonical `acg-box/rsnap` repository and final tag.
6. Treat any test, build, signing, timestamp, notarization, staple, Gatekeeper, appcast, checksum,
   upload, or draft-validation failure as a release blocker. Do not publish a package from a failed
   run.
7. The workflow does not publish crates.io packages or non-macOS desktop archives.
8. If publication succeeded but the final API response was lost, rerun the workflow. The publish
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
