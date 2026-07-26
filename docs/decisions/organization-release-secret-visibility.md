---
title: "Organization-Wide Release Secret Visibility"
description: "Why acg-box release secrets use organization visibility all while application trust anchors remain separate."
type: "Decision"
status: active
authority: normative
owner: acg-box/rsnap
last_verified: 2026-07-26
---
# Organization-Wide Release Secret Visibility

Status: accepted
Date: 2026-07-26

Context:

- `acg-box` is a single-operator GitHub organization.
- GitHub Actions supports repository, environment, and organization secrets. It does not support
  Actions secrets at the enterprise-account level.
- Configuring the same release credential in each repository creates repeated setup and cleanup
  work.
- Organization secret visibility `all` lets workflows in every current and future organization
  repository request the secret. A secret-name prefix does not enforce access control.
- A Sparkle private key is an application update trust anchor. Sharing one private key between
  applications would merge their update trust boundaries and could break safe key rotation.
- Current `main` still reads the generic repository secret `SPARKLE_PRIVATE_ED_KEY`. Promoting that
  legacy name to organization visibility `all` could make another application's workflow consume
  the Rsnap key by name.

Decision:

- Store reusable `acg-box` release credentials as GitHub organization secrets with visibility
  `all`.
- Treat every current and future `acg-box` repository and workflow as part of the organization
  credential boundary.
- Give each application a separate Sparkle private key and an application-specific organization
  secret name. Do not reuse the Rsnap key for another application.
- Do not add same-name repository or environment secrets because they can shadow the organization
  value.
- Do not create an organization secret for the generic legacy name `SPARKLE_PRIVATE_ED_KEY`.
- Keep the existing generic repository secret only during the observed migration window. Delete it
  after the `RSNAP_SPARKLE_PRIVATE_ED_KEY` consumer is on `main` and passes its acceptance check.
- Keep the value source in the operator's personal Infisical instance. GitHub-hosted workflows use
  the corresponding organization secret because they cannot reach the loopback Infisical service.

Alternatives considered:

- Restrict each organization secret to selected repositories.
  - Rejected because the single operator explicitly chose one organization-wide configuration for
    all repositories.
- Keep release credentials as repository secrets.
  - Rejected as the steady state because it repeats setup and cleanup for each repository.
- Reuse one Sparkle private key across applications.
  - Rejected because a Sparkle key is an application trust anchor, not a general signing service.
- Store GitHub Actions secrets at the enterprise-account level.
  - Rejected because GitHub Actions does not provide that secret scope.

Consequences:

- A new or compromised workflow in any `acg-box` repository can request organization release
  secrets. Repository creation and workflow review are security-boundary changes.
- Secret names must include the application identity when the value is application-specific.
- Credential rotation must update personal Infisical and the matching GitHub organization secret
  without changing another application's trust anchor.
- The temporary generic Rsnap repository secret is a cutover guard, not steady-state topology. Its
  removal closes the migration after the application-specific consumer lands.
- The current topology contract is `docs/spec/release-secret-topology.json`. The release contract
  is `docs/spec/release-distribution.md`.
