---
title: "Rsnap App Identity Contract"
description: "Normative requirements and invariants for Rsnap App Identity Contract."
type: "Spec"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-06
---
# Rsnap App Identity Contract

Purpose: Define the product display name, macOS app bundle name, and allowed stable technical
identifiers for Rsnap.

Status: normative

Read this when:

- Changing user-visible app names, Settings text, permission guidance, release packaging, or
  local macOS bundle staging.
- Auditing whether a lower-case `rsnap` occurrence is a branding miss or an intentional technical
  identifier.

Not this document:

- Use [`openwiki/reference/workspace-layout.md`](../reference/workspace-layout.md) for crate and directory ownership.
- Use [`openwiki/spec/telemetry.md`](./telemetry.md) for telemetry schema names and log predicates.

Defines:

- Product display name.
- macOS `.app` bundle name.
- Stable lower-case technical identifiers that must not be title-cased.

## Required user-visible names

- The product display name is `Rsnap`.
- The macOS app bundle is `Rsnap.app`.
- `CFBundleName` and `CFBundleDisplayName` must resolve to `Rsnap` for the staged native host
  bundle.
- Permission recovery drag chips and other UI affordances that represent the dragged bundle must
  show `Rsnap.app`.
- User-facing Settings, permission, onboarding, README, and runtime error text should use
  `Rsnap` when referring to the app or product.
- The default saved-capture filename prefix is `Rsnap`.

## Stable technical identifiers

Keep these identifiers lower-case unless a separate migration explicitly changes their technical
contract:

- Repository slug and URLs: `acg-box/rsnap`.
- Cargo package names, crate names, binary package selectors, and source paths such as
  `rsnap`, `rsnap-capture-core`, `rsnap-host-ffi`, and `apps/rsnap/`.
- Environment variables and build-time constants such as `RSNAP_NATIVE_HOST_STAGE_DIR`.
- Bundle identifier and preference domain: `ink.hack.rsnap`.
- Telemetry schemas and log predicates such as `rsnap.native_host.telemetry/1` and
  `rsnap.rust.telemetry/1`.
- C ABI symbols, headers, and Swift module bridge names containing `rsnap` or `Rsnap`.
- Local build directories such as `target/rsnap-native-host/`.

## Packaging invariant

Local staging, release packaging, and launcher fallback lookup must all use
`target/rsnap-native-host/Rsnap.app` unless `RSNAP_NATIVE_HOST_STAGE_DIR` overrides only the stage
directory. The override must still contain `Rsnap.app`, not `rsnap.app`.
