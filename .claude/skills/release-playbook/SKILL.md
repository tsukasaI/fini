---
name: release-playbook
description: >
  The verified release procedure for fini: version bump, tag push, the
  supply-chain-hardened crates.io publish flow (attest-before-publish,
  hash-drift check), and per-channel propagation (Homebrew, Nix, VS Code,
  pre-commit, GitHub Action). Use when: リリース準備, "publish new version",
  version bump, tagging a release, updating Homebrew/Nix/VS Code after a
  release. NOT for: routine CI failures on `main`, feature development,
  or anything that doesn't touch a `v*` tag.
allowed-tools: Read, Bash, Grep, Glob
---

# fini release playbook

SSoT for the publish flow is `.github/workflows/release.yaml` (three jobs:
`build` → `release` → `publish-crate`, ~177 lines total; `publish-crate` is
lines 103-177). This skill summarizes and points there — re-read the file
before acting, don't trust this summary as current.

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
2. Bump `version` in `Cargo.toml` (currently `0.3.0` — re-check before
   trusting this number). Then run `cargo build` (or `cargo update -p fini`
   equivalent) so `Cargo.lock` picks up the new version — `publish-crate`
   runs `cargo publish --locked --dry-run`, and `--locked` fails if the lock
   file is stale.
3. Commit the bump (`chore: bump vX.Y.Z`, matches history, e.g. `57c3c93`).
4. Tag `vX.Y.Z` matching `Cargo.toml` exactly and push the tag — this is
   the release trigger (`on.push.tags: v*` in `release.yaml`). The
   `publish-crate` job's `prepare` step hard-fails the job if
   `GITHUB_REF_NAME` (minus leading `v`) != the `Cargo.toml` version.

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
3. **`publish-crate`** (needs `release`) — the non-obvious part:
   - `cargo publish --locked --dry-run` — verifies the tag matches
     `Cargo.toml` version first; the dry-run also produces
     `target/package/fini-<version>.crate` as a side effect (replaces a
     separate `cargo package` step, deliberately dropped per commit
     `496a17e`'s follow-up — dry-run already validates the publish path).
   - Record `BEFORE` = sha256 of that `.crate`.
   - **Attest** the `.crate` (`actions/attest-build-provenance`) —
     attestation happens on the pre-publish file.
   - `cargo publish --locked --no-verify` — publish. `--no-verify` is used
     because `cargo publish` always re-packages internally regardless; the
     verify step would just rebuild redundantly (constraint documented in
     `496a17e`).
   - Record `AFTER` = sha256 of the same on-disk `.crate` path post-publish.
     If `BEFORE != AFTER`, the job fails loudly: the attestation may not
     cover the uploaded bytes, and the crate is already live — remediation
     is `cargo yank --version <v>`, then investigate (documented inline in
     the workflow).
   - `BEFORE == AFTER` is the proof that what was attested is what
     crates.io received.

All third-party actions are pinned to commit SHAs (not tags) — a deliberate
supply-chain decision (see `chore(deps)` commit adding Dependabot for
renewal). Don't "simplify" a SHA pin back to a tag. For the generic
SHA-pin refresh/audit procedure (enumerate pins, resolve tag → SHA, flag
stale pins), use the global `actions-pin-audit` skill
(`~/.claude/skills/actions-pin-audit/`) rather than re-deriving it here.

## Post-release propagation checklist (per channel)

| Channel | File | Action needed | Automatic? |
|---|---|---|---|
| crates.io | — | none | yes, via `publish-crate` |
| GitHub Release (tarballs + checksums.txt) | — | none | yes, via `release` job |
| Homebrew | `HomebrewFormula/fini.rb` | bump `version` + all 4 `sha256` values (macOS x86_64/arm64, Linux x86_64/arm64) from the new release's `checksums.txt` | **no — manual today.** `Refs: tsukasaI/fini#2` (open issue: automate via same-repo bump or separate tap repo) |
| Nix flake | `flake.nix` | bump `version = "X.Y.Z"` string only — `cargoLock.lockFile` points at `Cargo.lock`, no hash to update | no — manual, single-line edit |
| pre-commit hook | `.pre-commit-hooks.yaml` | none — `entry: fini` is unpinned, always resolves to whatever `fini` is on `PATH` | n/a, no version reference |
| GitHub Action | `action.yaml` | no version bump needed (resolves `latest` release or caller-pinned version at runtime); but reconsider the `verify-attestation` input default (`false`) — its own comment says flip callers to `true` "once your pinned fini version has attestations (releases after v0.3.0)", which is now true for every release | manual awareness, not a file edit |
| VS Code extension | `editors/vscode/package.json`, tag `vscode-v*` | **decoupled from the CLI.** Own version (currently `0.2.0`, independent of CLI's `0.3.0`) and own tag prefix/workflow (`release-vscode.yaml`). Do NOT bump per CLI release — only bump+tag when the extension itself changed | separate manual release, not part of this checklist unless the extension changed |

## Re-verify before relying on this file

This summarizes files as of the last edit — re-read before acting:

- `rg -n "^  (build|release|publish-crate):" .github/workflows/release.yaml`
  — confirm job names/order haven't changed.
- `rg '^version' Cargo.toml` — current version.
- `rg '"version"' editors/vscode/package.json` — current extension version.
- `gh issue view 2` — confirm the Homebrew automation issue is still open
  (not run for this file; run it yourself to re-check).
- `gh release list --limit 5` — see what's actually published (also not
  run here; a read command, but still left to the operator to invoke).
