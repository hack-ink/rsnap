---
title: "Release Distribution Contract"
description: "Required source, signing, artifact, and publication rules for Rsnap releases."
type: "Spec"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-28
---
# Release Distribution Contract

Purpose: Define the required trust and integrity properties of an Rsnap tag release.

Status: normative.

Read this when: You change the release workflow, macOS signing, Sparkle metadata, release assets,
or release credentials.

Not this document: Use `docs/runbook/release.md` for the operator sequence. Use
`docs/spec/app-identity.md` for product and bundle names.

Defines:

- accepted release source
- workflow permissions and jobs
- macOS signing and Personal Team distribution
- release assets and Sparkle metadata
- draft publication
- release secret scope

## Supported workflow shape

- Normal CI runs for pull requests, `main`, and merge queues.
- A formal release starts only when an authorized operator pushes a stable `vX.Y.Z` tag.
- The repository does not use a release-preparation, dry-run, or manual-dispatch release workflow.
- The release workflow must not create a tag.

## Source contract

- The tag is an annotated Git tag that points directly to a commit.
- The tag name is stable SemVer in the exact form `vX.Y.Z`, without leading zeroes.
- The tag commit, workflow event commit, and checked-out commit are the same commit.
- The tag commit is reachable from `origin/main`.
- The tag version matches `Cargo.toml` and the Rsnap workspace packages in `Cargo.lock`.
- The built app `CFBundleVersion` and `CFBundleShortVersionString` match the tag version.

## Permission contract

- The workflow default is `contents: read`.
- The macOS job does not have repository write permission.
- Only the Ubuntu publish job has `contents: write`.
- Checkout steps do not persist GitHub credentials.

## macOS package contract

- The package job runs on the standard GitHub-hosted ARM64 `macos-26` runner.
- The job runs the release test surface before it creates release assets.
- The app bundle name is `Rsnap.app`, and its bundle identifier is `ink.hack.rsnap`.
- The current release identity is the organization-provided Apple Development identity.
- Signing starts at the innermost Sparkle code and finishes with `Rsnap.app`.
- Each signed code object uses Hardened Runtime.
- `codesign --deep` can verify the final bundle. It must not sign the bundle.
- The Personal Team package is signed but not notarized.
- Users can need the documented Gatekeeper override after download.
- The final ZIP is created only after all inside-out signatures verify.

## Update and asset contract

Each release contains exactly these assets:

- `rsnap-aarch64-apple-darwin.zip`
- `appcast.xml`
- `rsnap-aarch64-apple-darwin.zip.sha256`

The appcast:

- uses the canonical `acg-box/rsnap` release and download URLs
- uses the release version from the validated tag
- records the exact ZIP byte length
- contains a Sparkle EdDSA signature made from the repository secret
  `RSNAP_SPARKLE_PRIVATE_ED_KEY`, passed to the signer as `SPARKLE_PRIVATE_ED_KEY`
- verifies with the public key embedded in `Rsnap.app`

The checksum file contains the lowercase SHA-256 digest of the final ZIP and the canonical ZIP
file name.

## Publication contract

- The macOS job must succeed before the Ubuntu publish job starts.
- The publish job uses the `release` environment as the final deployment approval and audit
  boundary. The environment stores no release secrets.
- The publisher validates local assets before it changes GitHub Release state.
- The publisher creates or reuses a draft and uploads only the three required assets.
- The publisher validates the exact remote asset set and downloaded bytes before publication.
- The last state-changing operation makes the draft public.
- A validation or upload failure leaves the release as a draft.
- The workflow does not publish crates.io packages or non-macOS desktop archives.

## Secret contract

The required Apple signing credentials are GitHub organization secrets with visibility `all`:

- `APPLE_CERTIFICATE_P12_BASE64`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`

`RSNAP_SPARKLE_PRIVATE_ED_KEY` is an Rsnap repository secret. It contains the private key that
matches `scripts/release/sparkle-public-ed-key.txt`. The Sparkle private key must not be an
organization or environment secret.

Repository and environment secrets must not shadow the Apple signing credential names. The
`release` environment stores no secrets.
