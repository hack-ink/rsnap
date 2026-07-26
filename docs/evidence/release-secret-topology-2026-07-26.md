---
title: "Release Secret Topology Evidence"
description: "Value-free provider and consumer evidence for the Rsnap release-secret migration."
type: "Migration Evidence"
status: active
authority: supporting
owner: acg-box/rsnap
last_verified: 2026-07-26
---

# Release Secret Topology Evidence

## Scope

This record supports `docs/spec/release-secret-topology.json` and
`docs/spec/release-distribution.md`. It contains identifiers, states, and test results. It does not
contain a secret value, access token, private key, certificate, or password.

Observation window: 2026-07-26T17:30:00Z through 2026-07-26T19:45:00Z.

## Personal Infisical Provider

- Domain: `http://127.0.0.1:51890`
- Upstream CLI: `/opt/homebrew/bin/infisical` version `0.43.114`
- Project: `rsnap-release`
- Project ID: `f55a1068-0ae7-4dee-a0c0-62bfe71016fc`
- Project delete protection: enabled
- Environment: `prod`
- Environment ID: `ddb4ad39-4b77-4d9e-a9bc-9acd4b5d0a71`
- Folder `/release/apple`: `93db9686-f2a8-4dde-806d-799a986efac5`
- Folder `/release/sparkle`: `4842bc7f-64ce-431a-ba83-2bab3bf84e6f`
- Machine identity: `rsnap-release-provisioner`
- Machine identity ID: `ac5f2cd1-4cfa-49a5-a9f4-9534a6111138`
- Project membership ID: `88a94de1-f62c-4cea-9320-912ea32d5d45`
- Universal Auth configuration ID: `26881d9a-5462-4c5c-98b5-fdab3ce26549`
- Universal Auth client ID: `dd88733b-1e26-43bf-b292-4062641d5e52`
- Universal Auth token TTL and maximum TTL: 900 seconds
- Universal Auth client-secret Keychain service and account:
  `infisical-rsnap-release` / `UA_CLIENT_SECRET`

Value-free access probes returned these results:

- Universal Auth login: allowed
- Rsnap project `/release` folder metadata: allowed
- Sibling project `9bc6a6dc-d0ce-424c-8bd2-3b277a8f77fe`: denied
- Exact key metadata for `/release/sparkle` / `RSNAP_SPARKLE_PRIVATE_ED_KEY`: present
- Private key derivation against the checked-in Rsnap Sparkle public key: pass

No Apple signing or notary value was provisioned. The `/release/apple` profile remains incomplete.
The provisioner currently has a project-member grant. The dedicated project contains only the two
declared release profiles. The time-bounded exception is recorded in the topology contract.

## GitHub Provider And Consumer

- Organization: `acg-box`
- Organization secret: `RSNAP_SPARKLE_PRIVATE_ED_KEY`
- Organization secret visibility: `all`
- Same-name repository secret: absent
- Same-name `release` environment secret: absent
- Existing compatibility repository secret: `SPARKLE_PRIVATE_ED_KEY`
- Obsolete repository secret `SPARKLE_PUBLIC_ED_KEY`: deleted
- Release environment ID: `18777671902`
- Required reviewer: `acgxv`; self-review is allowed
- Administrator environment bypass: enabled as an accepted single-operator recovery path
- Deployment policy: name `v*`, type `tag`, policy ID `55679605`
- Release-tag creation rule: `release-tags-authorized-creation`, active, only user `acgxv` can
  bypass creation
- Release-tag immutability rule: `release-tags-immutable`, active, no bypass for update or deletion

The one-time relay used a 4096-bit ephemeral RSA public key and RSA-OAEP with SHA-256. The relay
artifact contained ciphertext only. The local process decrypted the value, verified its derived
public key, and sent the same value to personal Infisical and the organization secret. GitHub
Actions canary run `30214599808` read the non-shadowed organization secret and passed the same
public-key derivation check. The relay artifact, relay run, canary run, local plaintext, local
ciphertext, and ephemeral RSA private key were then deleted.

## Legacy `hack-ink` Metadata

A value-free organization-secret listing found these `hack-ink` organization Actions secrets with
visibility `all`:

- `APPLE_CERTIFICATE_P12_BASE64`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`

GitHub does not permit reading their values. Current `hack-ink/decodex` documentation still
references these names. No Sparkle key or Team App Store Connect notary key was present in the
listing. The three names do not satisfy the six-name Rsnap Apple contract and their certificate
class, ownership, and current consumers were not proved. No value was copied and no `hack-ink`
secret was changed or deleted.

## Accepted Residual Risk

Organization visibility `all` lets workflows in every current and future `acg-box` repository
request the Rsnap Sparkle private key. The `RSNAP_` prefix prevents accidental name reuse but does
not enforce access isolation. The single operator accepted this organization-wide visibility. Each
application must still use a different Sparkle private-key name and value. New repositories and
workflow changes are part of the credential review boundary.

## Historical Release Boundary

A read-only audit of the public `v0.3.0` ZIP found this state:

- Bundle version: `0.3.0`
- Sparkle public key: matches the checked-in Rsnap key
- Feed URL: the historical `hack-ink/rsnap` URL, which currently redirects to
  `acg-box/rsnap`
- Apple signature: Apple Development, not Developer ID Application
- Stapled notarization ticket: absent
- Gatekeeper assessment: rejected
- Checksum release asset: absent

The historical asset does not satisfy the new release contract and was not replaced. Keeping the
same Sparkle public key preserves the archive-signature trust path for a later, higher-version
Developer ID release. The release-order validator also prevents a new draft from replacing
`v0.3.0` with an equal or lower stable version. Do not reuse the old `hack-ink/rsnap` repository
location while installed `v0.3.0` clients depend on its GitHub redirect for the update feed.

## Open Gates

- Current `main` still uses repository secret `SPARKLE_PRIVATE_ED_KEY`. Keep that rollback value
  until the new workflow lands on `main` and the organization-secret consumer is confirmed.
- Do not copy the generic compatibility name to organization scope. Another application's workflow
  could request that name and receive the Rsnap update key. Delete the repository secret after
  landing instead.
- Deleting the compatibility secret will make old workflow revisions that use the old name unable
  to rerun. This is an intentional cleanup tradeoff after landing.
- Six Apple organization secrets are absent. A paid Apple Developer Program team, a Developer ID
  Application certificate, and a Team App Store Connect API notary key are required.
- No real Developer ID signing, secure timestamp, notarization, staple, Gatekeeper assessment, tag,
  or GitHub Release was executed.

## Verdict

The personal Infisical provider, the application-specific GitHub organization secret, and the
non-shadowed canary consumer are `live-verified`. The migration remains open until the new consumer
lands on `main` and the exact legacy repository secret is deleted. Overall release readiness is also
blocked by the Apple credential gate.
