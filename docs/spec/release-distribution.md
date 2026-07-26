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

- The Release workflow starts only for an annotated tag with the exact form `vX.Y.Z`.
- Each SemVer component has no leading zero unless the component is `0`.
- Prerelease and build metadata are not accepted.
- The peeled tag commit, GitHub event object, and checked-out commit must resolve to the same
  commit.
- The tag commit must be reachable from `origin/main`.
- The tag version must equal `workspace.package.version` in `Cargo.toml` and each Rsnap workspace
  package version in `Cargo.lock`.
- The exact Sparkle version in `Package.swift` must equal the `Package.resolved` Sparkle pin.
- The workflow must not provide a release-preparation, dry-run, or manually dispatched release
  path.

## GitHub Runner and Permission Boundary

- The macOS package job uses the standard GitHub-hosted `macos-26` ARM64 runner.
- The workflow default permission is `contents: read`.
- Only the Ubuntu publication job has `contents: write`.
- The macOS package job must complete before the publication job starts.
- The publication checkout commit must equal the commit accepted by source validation.
- Release workflow runs are serialized and are not canceled in progress.
- GitHub Pages, dynamic CodeQL, required Dependency Review, Dependabot, and normal Language Checks
  keep their independent owners and permissions.

## Release Environment and Organization Secrets

Both the macOS package job and the Ubuntu publication job use the GitHub environment named
`release`. The environment must exist before a tag is pushed. Environment protection rules are an
operator-owned repository setting. The environment is the deployment protection boundary. It does
not own the long-lived release secrets.

The environment requires approval from GitHub user `acgxv`, permits self-review for the
single-operator repository, and has one custom deployment policy with name `v*` and type `tag`. The
workflow trigger and source validator apply the stricter exact `vX.Y.Z` rule.

Store the release credentials as GitHub Actions secrets in the single-operator `acg-box`
organization. Each secret must have visibility `all` so the operator's release configuration is
available to every repository in the organization. Do not create a repository or environment secret
with the same name. A narrower-scope secret would override the organization value and make the
active credential source ambiguous.

The macOS package job requires these non-empty organization secrets:

- `APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64`
- `APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD`
- `APPLE_DEVELOPER_ID_APPLICATION_IDENTITY`
- `APPLE_NOTARY_KEY_ID`
- `APPLE_NOTARY_ISSUER_ID`
- `APPLE_NOTARY_KEY_P8`
- `RSNAP_SPARKLE_PRIVATE_ED_KEY`

The notary credentials are a Team App Store Connect API key. The issuer ID is mandatory. Missing or
incomplete signing, notarization, or Sparkle credentials stop the package job.

`RSNAP_SPARKLE_PRIVATE_ED_KEY` is the Rsnap update trust anchor. Its organization-wide visibility
does not make it a shared application key. Only Rsnap release code can consume this name. The
private key must derive the checked-in Rsnap `SUPublicEDKey`. A key for another application is not
accepted, and another application must use a separately named private key. Do not define a
same-named repository or environment secret.

`SPARKLE_PUBLIC_ED_KEY` is not a required secret. The public key is checked in and shipped in the
application bundle.

Organization secrets are available to eligible workflows in each repository. GitHub does not bind
them to the `release` environment. Repository access control, protected release source validation,
and workflow review are therefore part of the credential boundary.

The personal Infisical instance at `http://127.0.0.1:51890` is the operator-controlled source record
for release material. Repository root `.infisical.json` pins project
`f55a1068-0ae7-4dee-a0c0-62bfe71016fc` and environment `prod`. Store the Apple values under
`/release/apple` and the Rsnap key under `/release/sparkle`. Use machine identity
`rsnap-release-provisioner` with explicit project, environment, and path arguments. Do not use a
personal CLI profile as automation authority.

GitHub-hosted runners cannot reach this loopback Infisical instance. The Release workflow therefore
uses GitHub organization secrets at run time. When a release credential is created or rotated,
write the same value to the pinned Infisical path and its GitHub organization secret in one change
window. Verify both consumers before removing the previous value. Never copy values to a company
Infisical instance or to another application's Sparkle secret.

The package script must remove all Apple and Sparkle secret names from the child-process
environment before it starts the Rust and Swift build. It must finish the unsigned build and bundle
validation before it writes the P12 or P8 file or creates the release keychain. It passes a secret
only to the command that requires that secret.

## macOS Test, Build, and Bundle Contract

- The macOS job runs credential-free release-script tests, Rust tests with the `final-release`
  profile, and release-configuration Swift host probes before packaging.
- Rust uses the `final-release` profile and Swift uses the release configuration for the packaged
  app.
- Bundle assembly copies `Sparkle.framework` only from the selected Swift build output.
- The staged bundle and final ZIP must contain the Sparkle version that is locked in
  `Package.resolved`.
- `CFBundleShortVersionString`, `CFBundleVersion`, the Sparkle appcast versions, and the tag version
  must match the Cargo release version.

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

- Sparkle `sign_update` signs the final stapled ZIP and verifies its own Ed25519 signature.
- The appcast signature must decode to 64 bytes.
- The appcast enclosure length must equal the final ZIP byte length.
- `sparkle:minimumSystemVersion` is `14.0.0` and `sparkle:hardwareRequirements` is `arm64`.
- The checksum file contains the lowercase SHA-256 digest of the final ZIP and its canonical file
  name.

## Draft Publication Contract

- The Ubuntu job creates or reuses a draft for the validated tag.
- Before draft creation, the job confirms that the remote annotated tag still points directly to
  the commit accepted by source validation.
- The job repeats that remote tag check immediately before it publishes the draft.
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
