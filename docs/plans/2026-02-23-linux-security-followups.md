# Linux Security Follow-ups (Deferred)

This note records a Linux-specific security issue discovered while the project target is **macOS + Windows**.
No Linux implementation work is planned right now, but this should be revisited before enabling Linux support.

## What happened

- GitHub Dependabot flagged `glib` (Rust) advisory `GHSA-wrw7-89jp-8q8g` as a **medium** severity alert, with `Cargo.lock` as the manifest.
- The vulnerable `glib` version is pulled in through the Linux GTK3 backend stack used by `tauri`/`wry` (e.g. `webkit2gtk`, `gtk`), and is not part of the dependency graph on macOS/Windows targets.
- At the time of writing (2026-02-23), `tauri` on crates.io is `2.10.2`, and the Linux GTK3 stack depends on `gtk = 0.18.*`, which in turn requires `glib = 0.18.*`. This prevents upgrading `glib` to `0.20.0` (the first patched version) without upstream changes.

## Current disposition

- The Dependabot alert was dismissed as `not_used` with a comment noting that the project currently targets macOS + Windows only.
- This is intentionally *not* a permanent “fix”; it is a deferral until Linux support is actually enabled.

## Before enabling Linux support

1. Re-check the upstream `tauri`/`wry` Linux backend dependency chain and see whether it has moved off GTK3 (`gtk 0.18`) to a stack that can use `glib >= 0.20.0`.
2. Remove/don’t rely on the previous dismissal and ensure the repository is clean of open security alerts on the default branch.
3. Re-run audit and dependency inspection for the Linux target(s).

Useful commands:

- Dependency chain (all targets): `cargo tree -i glib --target all --workspace`
- Linux-only builds: use `--target x86_64-unknown-linux-gnu` (and other intended triples) and re-run `cargo tree`/`cargo audit`.
