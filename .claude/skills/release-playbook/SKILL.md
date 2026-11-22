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

SSoT for the release flow is `.github/workflows/release.yaml` (four jobs:
`verify` → `build` → `release` → `update-homebrew-formula`, the last one
calling `.github/workflows/homebrew-formula.yaml`). fini is deliberately not
published to crates.io (`publish = false` in `Cargo.toml`; the earlier
`publish-crate` job never succeeded and was removed). This skill summarizes
and points there. Re-read the files before acting; don't trust this summary
as current.

Architecture reference: `CLAUDE.md` in repo root. Don't duplicate it here.

## When NOT to use

- CI is red on `main` but no tag is involved: that's a normal debugging task.
- Adding a feature/fixing a bug: this skill starts once the code is ready
  to ship, not before.

## Pre-release checklist (verified from files)

1. `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings`,
   `cargo fmt --check` all pass locally (mirrors `ci.yaml` jobs
   `test`/`clippy`/`fmt`; CI runs `test` on both Ubuntu and macOS). CI's
   `audit` job runs via the `rustsec/audit-check` action, not a bare command;
   the local equivalent is `cargo audit`.
2. Bump `version` in `Cargo.toml` (check the current value before trusting
   any number cited elsewhere in this file). Then run `cargo build` so
   `Cargo.lock` picks up the new version, and commit the updated
   `Cargo.lock` with the bump: `verify` runs `cargo test --locked` and
   `build` runs `cargo build --release --locked`, so a stale lock fails the
   release.
3. Update the version strings that are synced by hand: `flake.nix`
   (`version = "X.Y.Z"`), and in `README.md` the `cargo install ... --tag`
   lines, the pre-commit `rev:`, and the GitHub Action examples
   (`tsukasaI/fini@vX.Y.Z`, `version: 'vX.Y.Z'`).
   `rg -n 'v?[0-9]+\.[0-9]+\.[0-9]+' README.md flake.nix` lists the
   candidates.
4. Main takes changes through pull requests only (ruleset, no bypass): put
   the bump on a branch (`chore: bump vX.Y.Z`, matches history, e.g.
   `57c3c93`), open a PR, and squash-merge it.
5. Tag `vX.Y.Z` on the **merged commit on `main`**, not on the branch
   commit, and push the tag. This is the release trigger (`on.push.tags:
   v*`). `verify` fails a tag whose commit is not on `main`, and `build`
   fails a tag that does not match the `Cargo.toml` version.
6. A tag containing `-` (`v1.0.0-rc.1`) is published as a pre-release, is
   not the "latest" release the GitHub Action resolves, and does not update
   the Homebrew formula.

## Release steps (what `v*` push triggers)

Per `.github/workflows/release.yaml`:

1. **`verify`**: checks that the tagged commit is an ancestor of
   `origin/main`, then runs `cargo test --locked`.
2. **`build`** (needs `verify`): checks the tag against the `Cargo.toml`
   version, cross-builds 4 targets (macOS x86_64/arm64 on `macos-latest`,
   Linux x86_64/arm64 on `ubuntu-24.04`, pinned so the minimum glibc does
   not move) with `cargo build --release --locked`, tars each as
   `fini-<target>.tar.gz`, uploads as artifacts.
3. **`release`** (needs `build`): downloads all tarballs, generates
   `checksums.txt` (sha256sum), attests build provenance on the tarballs
   (`actions/attest-build-provenance`), creates the GitHub Release via
   `softprops/action-gh-release` with the tarballs + checksums attached and
   generated release notes; `prerelease` is set when the tag contains `-`.
4. **`update-homebrew-formula`** (needs `release`, skipped for pre-release
   tags): calls `homebrew-formula.yaml`, which validates the tag (`vX.Y.Z`
   only), downloads the release assets, regenerates
   `HomebrewFormula/fini.rb` via `scripts/update-homebrew-formula.sh`
   (every tarball must be listed in `checksums.txt` and verified, fail
   closed) and **opens a pull request** `chore/homebrew-formula-X.Y.Z`. It
   does not push to `main`. If the formula is already current it prints
   "Formula already up to date" and exits 0.

The formula workflow can also be run by hand for an existing release:
`gh workflow run homebrew-formula.yaml -f tag=vX.Y.Z`. A re-run fails at
`git push` if `chore/homebrew-formula-X.Y.Z` already exists on the remote;
delete that branch first.

There is no crates.io publish step: `Cargo.toml` sets `publish = false`, so
an accidental `cargo publish` is refused. Publishing would need the job
(with its attest-before-publish and hash-drift check) restored from git
history plus a working `CARGO_REGISTRY_TOKEN`.

All third-party actions are pinned to commit SHAs (not tags), a deliberate
supply-chain decision (see `chore(deps)` commit adding Dependabot for
renewal). Don't "simplify" a SHA pin back to a tag. For the generic
SHA-pin refresh/audit procedure (enumerate pins, resolve tag → SHA, flag
stale pins), use the global `actions-pin-audit` skill
(`~/.claude/skills/actions-pin-audit/`) rather than re-deriving it here.

## Post-release propagation checklist (per channel)

| Channel | File | Action needed | Automatic? |
|---|---|---|---|
| crates.io | n/a | not published (`publish = false`) | n/a |
| GitHub Release (tarballs + checksums.txt) | n/a | review the generated notes and add a BREAKING section by hand for behavior changes (`gh release edit vX.Y.Z --notes-file ...`) | release yes, BREAKING notes no |
| Homebrew | `HomebrewFormula/fini.rb` | review and merge the `chore(homebrew): update formula to X.Y.Z` PR the workflow opens | PR yes, merge no |
| Nix flake | `flake.nix` | `version = "X.Y.Z"` bumped before tagging (checklist step 3); `cargoLock.lockFile` points at `Cargo.lock`, no hash to update | no, manual single-line edit |
| pre-commit hook | `.pre-commit-hooks.yaml` | none: both hooks are `language: system`, `entry: fini`, so they run whatever `fini` is on `PATH`; users' `rev:` pins only the definition. README's `rev:` example is bumped in checklist step 3 | n/a, no version reference |
| GitHub Action | `action.yaml` | no version bump needed (resolves `latest` release or caller-pinned version at runtime; `version` accepts `X.Y.Z` or `vX.Y.Z`); `verify-attestation` defaults to `true` (attestations exist for every release from v0.4.0 onward), and callers pinning a release before v0.4.0 must set it to `false`. README's Action examples are bumped in checklist step 3 | manual awareness, not a file edit |

## Re-verify before relying on this file

This summarizes files as of the last edit. Re-read before acting:

- `rg -n "^  (verify|build|release|update-homebrew-formula):" .github/workflows/release.yaml`
  to confirm job names/order haven't changed.
- `rg '^version' Cargo.toml` for the current version.
- `gh release list --limit 5` to see what's actually published.
- `gh pr list --search "chore(homebrew)"` for the formula PR from the last
  release, if not merged yet.
