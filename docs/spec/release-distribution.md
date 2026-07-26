---
title: "Release Distribution Contract"
description: "Required source, trust, artifact, and publication boundaries for Rsnap releases."
type: "Spec"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-26
---
# Release Distribution Contract

Purpose: Define the source, signing, notarization, update, and GitHub publication requirements for
an Rsnap macOS release.

Status: normative

Read this when: You change `.github/workflows/release.yml`, the macOS bundle assembly path,
release scripts, Sparkle metadata, release credentials, or release validation.

Not this document: Use `docs/runbook/validate-release.md` for operator steps. Use
`docs/spec/app-identity.md` for product and bundle names.

Defines:

- accepted release source;
- macOS distribution trust requirements;
- release artifact names and metadata;
- GitHub environment and organization secret contract;
- draft-to-public transition.

## Release Entry Point

- A tag name that matches the workflow's candidate filter can start a Release run. Source
  validation permits the run to continue only for an annotated tag with the exact form `vX.Y.Z`.
- Each SemVer component has no leading zero unless the component is `0`.
- Prerelease and build metadata are not accepted.
- The peeled tag commit, GitHub event object, and checked-out commit must resolve to the same
  commit.
- The tag commit must equal the current `origin/main` tip. An older commit that remains reachable
  from `main` is not an accepted release source.
- The tag version must equal `workspace.package.version` in `Cargo.toml` and each Rsnap workspace
  package version in `Cargo.lock`.
- The exact Sparkle version in `Package.swift` must equal the `Package.resolved` Sparkle pin.
- The workflow must not provide a release-preparation, dry-run, or manually dispatched release
  path.
- GitHub control-plane rules restrict creation of a matching `v*` tag to user `acgxv`. A separate
  rule forbids every actor from updating or deleting a matching tag after creation. These repository
  rules are additive to the enterprise rules that require signed tag history.

## GitHub Runner and Permission Boundary

- The credential-free macOS build job and the fresh macOS signing job use separate standard
  GitHub-hosted `macos-26` ARM64 runners.
- The workflow default permission is `contents: read`.
- Only the Ubuntu publication job has `contents: write`.
- The signing job starts only after the credential-free build uploads a validated unsigned app
  handoff. It does not reuse the build runner.
- The macOS signing job must complete before the publication job starts.
- The publication checkout commit must equal the commit accepted by source validation.
- Release workflow runs are serialized and are not canceled in progress.
- GitHub Pages, dynamic CodeQL, required Dependency Review, Dependabot, and normal Language Checks
  keep their independent owners and permissions.

## Release Environment and Organization Secrets

The fresh macOS signing job and the Ubuntu publication job use the GitHub environment named
`release`. The credential-free test and build job does not use this environment. The environment
must exist before a tag is pushed. Environment protection rules are an operator-owned repository
setting. The environment is the deployment protection boundary. It does not own the long-lived
release secrets.

The normal environment deployment path requires approval from GitHub user `acgxv` and permits
self-review for the single-operator repository. Repository administrators can force a deployment
past the environment protection rules. This administrator recovery path is an accepted
single-operator control-plane exception and does not bypass the workflow's source or artifact
validation. The environment has one deployment branch/tag policy with name `v*` and type `tag`.
The workflow candidate filter narrows tag names. The source validator applies the exact canonical
`vX.Y.Z` rule.

Store the release credentials as GitHub Actions secrets in the single-operator `acg-box`
organization. Each secret must have visibility `all` so the operator's release configuration is
available to every repository in the organization. Do not create a repository or environment secret
with the same name. A narrower-scope secret would override the organization value and make the
active credential source ambiguous.

The fresh macOS signing job requires these non-empty organization secrets:

- `APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64`
- `APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD`
- `APPLE_DEVELOPER_ID_APPLICATION_IDENTITY`
- `APPLE_NOTARY_KEY_ID`
- `APPLE_NOTARY_ISSUER_ID`
- `APPLE_NOTARY_KEY_P8`
- `RSNAP_SPARKLE_PRIVATE_ED_KEY`

The notary credentials are a Team App Store Connect API key. An individual key is not accepted.
The key ID is a 10-character uppercase alphanumeric value, and the issuer ID is a UUID. The P12
value is one RFC 4648 base64 value without whitespace. Missing, malformed, or incomplete signing,
notarization, or Sparkle credentials stop the signing job.

`RSNAP_SPARKLE_PRIVATE_ED_KEY` is the Rsnap update trust anchor. Its organization-wide visibility
does not make it a shared application key. The Rsnap Release workflow is the intended consumer, but
any workflow in any current or future `acg-box` repository can request the secret because visibility
is `all`. The `RSNAP_` prefix is a naming boundary, not access control. This cross-repository exposure
is an accepted single-operator organization tradeoff. The private key must derive the checked-in
Rsnap `SUPublicEDKey`. A key for another application is not accepted, and another application must
use a separately named private key. Do not define a same-named repository or environment secret.
Review every new organization repository and workflow change as part of this credential boundary.

`SPARKLE_PUBLIC_ED_KEY` is not a required secret. The public key is checked in and shipped in the
application bundle.

The generic repository secret `SPARKLE_PRIVATE_ED_KEY` is a temporary compatibility input for the
workflow on `main` before this contract lands. Do not promote that generic name to organization
visibility because another application's workflow could consume the Rsnap key. Delete the
repository secret after the application-specific organization-secret consumer lands on `main`.

Organization secrets are available to eligible workflows in each repository. GitHub does not bind
them to the `release` environment. Repository access control, protected release source validation,
and workflow review are therefore part of the credential boundary.

The personal Infisical instance at `http://127.0.0.1:51890` is the operator-controlled source record
for release material. `docs/spec/release-secret-topology.json` is the version 2 desired-state
contract. Repository root `.infisical.json` is routing only; it pins project
`f55a1068-0ae7-4dee-a0c0-62bfe71016fc` and environment `prod` and must agree with the topology
contract. Store the Apple values under `/release/apple` and the Rsnap key under
`/release/sparkle`. Use machine identity `rsnap-release-provisioner` with explicit domain, project,
environment, and path arguments. Do not use a personal CLI profile as automation authority. The
value-free provider evidence is in `docs/evidence/release-secret-topology-2026-07-26.md`.

GitHub-hosted runners cannot reach this loopback Infisical instance. The Release workflow therefore
uses GitHub organization secrets at run time. When a release credential is created or rotated,
write the same value to the pinned Infisical path and its GitHub organization secret in one change
window. Verify both consumers before removing the previous value. Never copy values to a company
Infisical instance or to another application's Sparkle secret.

The credential-free job must complete all repository tests, dependency resolution, Rust and Swift
builds, app assembly, and unsigned-bundle validation without a release environment or release
secret. A fresh runner downloads the unsigned handoff by its immutable artifact ID, verifies the
expected SHA-256 digest and release metadata before extraction, and verifies the extracted app
again. The secret-bearing step fixes `BASH_ENV` and `ENV` to `/dev/null`. It must not run Cargo,
SwiftPM, a dependency-provided tool, or code from the unsigned app. It passes a secret only to the
audited signing, key-validation, notarization, and appcast operations that require that secret.

## macOS Test, Build, and Bundle Contract

- The credential-free macOS job runs release-script tests, Rust tests with the `final-release`
  profile, and release-configuration Swift host probes before packaging.
- Rust uses the `final-release` profile and Swift uses the release configuration for the packaged
  app.
- Bundle assembly copies `Sparkle.framework` only from the selected Swift build output.
- The staged bundle and final ZIP must contain the Sparkle version that is locked in
  `Package.resolved`.
- `CFBundleShortVersionString`, `CFBundleVersion`, the Sparkle appcast versions, and the tag version
  must match the Cargo release version.
- The unsigned handoff is a ZIP that preserves the app's executable modes and fixed Sparkle
  symlink graph. Validation rejects unsafe paths, hidden paths, duplicate entries, encrypted
  entries, special files, special permission bits, unexpected symlinks, unexpected executables,
  excessive entry counts, excessive expanded size, CRC failure, metadata mismatch, and digest
  mismatch before extraction.
- The fresh signing job runs no build or dependency-resolution command. It signs only the validated
  app from the handoff.

## Signing and Notarization Contract

- The release identity is an exact `Developer ID Application` identity.
- All executable code uses Hardened Runtime and a secure timestamp.
- The fixed Sparkle 2.9.4 code graph is signed from the inside out in this order:
  `Installer.xpc`, `Downloader.xpc`, `Autoupdate`, `Updater.app`, `Sparkle.framework`, and
  `Rsnap.app`.
- Only `Downloader.xpc` preserves its upstream entitlements during re-signing.
- The outer release app does not inherit SwiftPM development entitlements. It must not contain
  `get-task-allow` or disabled library validation.
- A signing command must not use `codesign --deep`. A final recursive verification command may use
  `--deep`.
- Each signed code object must report the same non-empty Team ID, Developer ID Application
  authority, Hardened Runtime, and a secure timestamp.
- Apple notarization is mandatory. The `notarytool` result must contain the terminal status
  `Accepted`.
- The upload command must first return a valid notarization submission UUID. The workflow prints
  this UUID and waits for that exact submission. A wait failure or timeout prints the UUID and stops
  the release so an operator can inspect the continuing Apple submission.
- The app is stapled only after notarization succeeds. `stapler validate`, recursive code-signature
  verification, and `spctl --assess --type execute` must then succeed.
- The final public ZIP is created only after all signing, notarization, staple, and Gatekeeper
  checks succeed.

## Artifact and Sparkle Contract

The release contains exactly these uploaded assets:

- `rsnap-aarch64-apple-darwin.zip`
- `appcast.xml`
- `rsnap-aarch64-apple-darwin.zip.sha256`

The appcast enclosure URL is
`https://github.com/acg-box/rsnap/releases/download/vX.Y.Z/rsnap-aarch64-apple-darwin.zip`.
The bundle feed URL is
`https://github.com/acg-box/rsnap/releases/latest/download/appcast.xml`.

- The checked-in CryptoKit signer signs the final stapled ZIP with Ed25519 and verifies the new
  signature before it returns it. The signing job must not execute Sparkle `sign_update` or another
  binary supplied by the build artifact or dependency cache.
- The first release under this contract changes the historical Apple signing class from Apple
  Development to Developer ID Application. It must retain the existing Rsnap Ed25519 key. Do not
  rotate the Apple signing identity and the Sparkle key in the same release.
- The appcast signature must decode to 64 bytes.
- The appcast enclosure length must equal the final ZIP byte length.
- `sparkle:minimumSystemVersion` is `14.0.0` and `sparkle:hardwareRequirements` is `arm64`.
- The checksum file contains the lowercase SHA-256 digest of the final ZIP and its canonical file
  name.

## Draft Publication Contract

- The Ubuntu job creates or reuses a draft for the validated tag.
- Before any draft creation or upload, the job reads a fail-closed inventory of public and draft
  releases. A new or existing draft version must be strictly greater than the highest public stable
  SemVer. A prerelease is not an accepted stable release.
- GitHub's latest pointer must identify the highest public stable SemVer. An incomplete inventory or
  an incoherent latest pointer stops publication.
- Before draft creation, the job confirms that the remote annotated tag still points directly to
  the commit accepted by source validation.
- The job repeats that remote tag check immediately before it publishes the draft.
- The job repeats the release inventory check immediately before publication. It uses GitHub's
  legacy SemVer-aware latest selection so a concurrently published higher version cannot be
  replaced as latest.
- The job must not modify an already public release. A retry may validate an existing public
  release and finish successfully without a mutation.
- The job uploads only the three canonical assets.
- While the release is still a draft, the job validates the exact asset set, API size, optional API
  digest, repository URL, appcast metadata, checksum, the Ed25519 signature against the checked-in
  Rsnap public key, and authenticated downloaded bytes. GitHub can use one consistent temporary
  `untagged-*` slug in draft API URLs. The appcast URLs must still use the canonical repository and
  final tag.
- For a public retry, the job downloads and validates the public assets directly. It does not
  compare them with a new local build because signing timestamps, notarization tickets, appcast
  dates, and ZIP metadata are not reproducible.
- Any failure before publication leaves the release as a draft and therefore not publicly visible.
- Changing `draft` to `false` is the final mutation in the draft path.
