# Rsnap OpenWiki Brief

## Scope

- Make `openwiki/` the only maintained repository knowledge and agent-routing surface.
- Treat the existing `docs/` tree as migration input. Preserve as much useful technical
  content as possible before removing `docs/`.
- Preserve the current authority classes and their boundaries: normative specifications,
  procedural runbooks, current-state references, accepted decisions, research provenance,
  drift evidence, and the documentation log.
- Keep the existing subject-specific pages when consolidation would lose requirements,
  commands, rationale, evidence, history, or retrieval precision.

## OpenWiki Structure

- Follow Open Knowledge Format v0.1.
- Use `openwiki/quickstart.md` as the canonical agent entry point.
- Keep `openwiki/index.md` as the OKF root index with `okf_version: "0.1"`.
- Keep `index.md` and `log.md` as reserved documents. Give every other generated concept
  Markdown file YAML front matter with a non-empty `type`.
- Nested `spec/`, `runbook/`, `reference/`, `decisions/`, `research/`, and `evidence/`
  sections are intentional. They preserve established claim ownership in this large corpus.
- Use stable relative Markdown links and exact repository paths.

## Authority And Maintenance

- User instructions and checked-in project policy come first. Source code, tests,
  configuration, manifests, workflows, and observed runtime behavior own current behavior.
  Specifications and runbooks own their declared contracts and procedures. OpenWiki is the
  retrieval and maintenance layer; it does not override a higher-authority source.
- Keep one canonical owner for each durable claim. Link to it instead of duplicating it.
- Maintain the wiki generator-first: update the source that owns a claim, run
  `openwiki code --update --print`, and review the generated diff against source authority.
  Direct page edits are only for explicit curation or correction.
- Do not add or authorize recurring OpenWiki automation. Repository automation requires a
  separate checked-in decision and correctly routed credentials.
- After every code-mode run, review the OpenWiki blocks in root `AGENTS.md` and `CLAUDE.md`.
  OpenWiki v0.2.3 can restore generic scheduled-workflow wording even when no such workflow is
  authorized or present. Keep those blocks aligned with the repository's manual,
  generator-first policy.
- Never include secrets, credentials, private user data, local environment contents, or
  machine-specific secret-routing details.

## Writing

- Write durable and executable content in English.
- Use concise technical English and preserve exact product names, identifiers, commands,
  paths, schemas, event names, units, dates, measured values, and historical claims.
- Do not silently modernize or rewrite research evidence, audit results, accepted decision
  history, or prior measurements during migration.
