---
name: release-playbook
description: >
  The verified release procedure for fini: version bump, tag push, the
  supply-chain-hardened GitHub Release flow (tarball provenance
  attestation, checksums), and per-channel propagation (Homebrew, Nix,
  pre-commit, GitHub Action). fini is not published to crates.io. Use when: リリース準備, "publish new version",
  version bump, tagging a release, updating Homebrew/Nix after a
  release. NOT for: routine CI failures on `main`, feature development,
  or anything that doesn't touch a `v*` tag.
allowed-tools: Read, Bash, Grep, Glob
---

# fini release playbook

SSoT for the release flow is `.github/workflows/release.yaml` (three jobs:
`build` → `release` → `update-homebrew-formula`). fini is deliberately not
published to crates.io (`publish = false` in `Cargo.toml`; the earlier
`publish-crate` job never succeeded and was removed). This skill summarizes
and points there — re-read the file before acting, don't trust this summary
as current.

Architecture reference: `CLAUDE.md` in repo root. Don't duplicate it here.

## When NOT to use

- CI is red on `main` but no tag is involved — that's a normal debugging task.
- Adding a feature/fixing a bug — this skill starts once the code is ready
  to ship, not before.

## Pre-release checklist (verified from files)

1. `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` all pass
   locally (mirrors `ci.yaml` jobs `test`/`clippy`/`fmt`). CI's `audit` job
   runs via the `rustsec/audit-check` action, not a bare command; the local
   equivalent is `cargo audit`.
2. Bump `version` in `Cargo.toml` (check the current value before trusting
   any number cited elsewhere in this file). Then run `cargo build` (or
   `cargo update -p fini` equivalent) so `Cargo.lock` picks up the new
   version, and commit the updated `Cargo.lock` with the bump (the `build`
   job runs plain `cargo build --release`, not `--locked`, so a stale lock
   would not fail CI).
3. Commit the bump (`chore: bump vX.Y.Z`, matches history, e.g. `57c3c93`).
4. Tag `vX.Y.Z` matching `Cargo.toml` exactly and push the tag — this is
   the release trigger (`on.push.tags: v*` in `release.yaml`). No job
   checks the tag against the `Cargo.toml` version any more (only the tag
   format is validated, in `update-homebrew-formula`), so verify the match
   yourself before pushing.

**未実行・ファイル根拠で検証**: no build/test/tag/publish command above was
run in producing this file; each step is read from `ci.yaml` / `release.yaml`
/ `Cargo.toml` as cited.

## Release steps (what `v*` push triggers)

Per `.github/workflows/release.yaml`:

1. **`build`** — cross-builds 4 targets (macOS x86_64/arm64, Linux
   x86_64/arm64 via `dtolnay/rust-toolchain`), tars each as
   `fini-<target>.tar.gz`, uploads as artifacts.
2. **`release`** (needs `build`) — downloads all tarballs, generates
   `checksums.txt` (sha256sum), attests build provenance on the tarballs
   (`actions/attest-build-provenance`), creates the GitHub Release via
   `softprops/action-gh-release` with the tarballs + checksums attached.
3. **`update-homebrew-formula`** (needs `release`) — validates the tag
   format, downloads the release assets, regenerates
   `HomebrewFormula/fini.rb` (checksums verified, fail closed) and pushes
   it to `main`.

There is no crates.io publish step: `Cargo.toml` sets `publish = false`, so
an accidental `cargo publish` is refused. Publishing would need the job
(with its attest-before-publish and hash-drift check) restored from git
history plus a working `CARGO_REGISTRY_TOKEN`.

All third-party actions are pinned to commit SHAs (not tags) — a deliberate
supply-chain decision (see `chore(deps)` commit adding Dependabot for
renewal). Don't "simplify" a SHA pin back to a tag. For the generic
SHA-pin refresh/audit procedure (enumerate pins, resolve tag → SHA, flag
stale pins), use the global `actions-pin-audit` skill
(`~/.claude/skills/actions-pin-audit/`) rather than re-deriving it here.

## Post-release propagation checklist (per channel)

| Channel | File | Action needed | Automatic? |
|---|---|---|---|
| crates.io | — | not published (`publish = false`) | n/a |
| GitHub Release (tarballs + checksums.txt) | — | none | yes, via `release` job |
| Homebrew | `HomebrewFormula/fini.rb` | none — the `update-homebrew-formula` job in `release.yaml` downloads the release assets, verifies `checksums.txt`, regenerates the formula via `scripts/update-homebrew-formula.sh`, and pushes to `main`. After a release, `git pull` and confirm the bot commit landed | yes, via `update-homebrew-formula` (tsukasaI/fini#2) |
| Nix flake | `flake.nix` | bump `version = "X.Y.Z"` string only — `cargoLock.lockFile` points at `Cargo.lock`, no hash to update | no — manual, single-line edit |
| pre-commit hook | `.pre-commit-hooks.yaml` | none — `entry: fini` is unpinned, always resolves to whatever `fini` is on `PATH` | n/a, no version reference |
| GitHub Action | `action.yaml` | no version bump needed (resolves `latest` release or caller-pinned version at runtime); `verify-attestation` defaults to `true` (attestations exist for every release from v0.4.0 onward) - callers pinning a release before v0.4.0 must set it to `false` | manual awareness, not a file edit |

## Re-verify before relying on this file

This summarizes files as of the last edit — re-read before acting:

- `rg -n "^  (build|release|update-homebrew-formula):" .github/workflows/release.yaml`
  — confirm job names/order haven't changed.
- `rg '^version' Cargo.toml` — current version.
- `gh issue view 2` — confirm the Homebrew automation issue is still open
  (not run for this file; run it yourself to re-check).
- `gh release list --limit 5` — see what's actually published (also not
  run here; a read command, but still left to the operator to invoke).
