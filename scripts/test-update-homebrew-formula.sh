#!/usr/bin/env bash
# Regression test for scripts/update-homebrew-formula.sh's version validation
# (issue #90: a malicious/malformed tag name must never reach the Ruby
# heredoc that embeds it into HomebrewFormula/fini.rb).
#
# Assets are built as real (valid) release assets so the run reaches the
# version-embedding step for every case; without the validation fix this
# test fails because the malicious cases would successfully rewrite the
# formula with injected content.
#
# Run manually: bash scripts/test-update-homebrew-formula.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/update-homebrew-formula.sh"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/fini-homebrew-test.XXXXXX")"
trap 'rm -rf "$WORKDIR"' EXIT

if command -v sha256sum >/dev/null 2>&1; then
  SHA256() { sha256sum "$@"; }
else
  SHA256() { shasum -a 256 "$@"; }
fi

# Isolated fake repo root so a failed run can never touch the real
# HomebrewFormula/fini.rb.
FAKE_REPO="$WORKDIR/repo"
mkdir -p "$FAKE_REPO/scripts" "$FAKE_REPO/HomebrewFormula"
cp "$SCRIPT" "$FAKE_REPO/scripts/update-homebrew-formula.sh"
FORMULA="$FAKE_REPO/HomebrewFormula/fini.rb"

ASSETS_DIR="$WORKDIR/assets"
mkdir -p "$ASSETS_DIR"
for target in x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
  echo "fake binary for $target" > "$ASSETS_DIR/fini-$target.tar.gz"
done
(cd "$ASSETS_DIR" && SHA256 fini-*.tar.gz > checksums.txt)

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

reset_formula() {
  echo "sentinel: pre-existing content" > "$FORMULA"
}

run_script() {
  (cd "$FAKE_REPO" && bash scripts/update-homebrew-formula.sh "$1" "$ASSETS_DIR")
}

# --- Malicious tag-shaped argument must be rejected (exit 2), formula untouched ---
reset_formula
MALICIOUS='0.5.0"; echo pwned; x="'
set +e
run_script "$MALICIOUS" >/dev/null 2>&1
STATUS=$?
set -e
[ "$STATUS" -eq 2 ] || fail "malicious version '$MALICIOUS' did not exit 2 (got $STATUS)"
[ "$(cat "$FORMULA")" = "sentinel: pre-existing content" ] || fail "formula file was modified by malicious version '$MALICIOUS'"
echo "PASS: malicious version (quote injection) rejected with exit 2, formula untouched"

# --- Another injection shape (semicolon + backtick) ---
reset_formula
MALICIOUS2='1.0.0; rm -rf /`'
set +e
run_script "$MALICIOUS2" >/dev/null 2>&1
STATUS=$?
set -e
[ "$STATUS" -eq 2 ] || fail "malicious version '$MALICIOUS2' did not exit 2 (got $STATUS)"
[ "$(cat "$FORMULA")" = "sentinel: pre-existing content" ] || fail "formula file was modified by malicious version '$MALICIOUS2'"
echo "PASS: malicious version (shell metacharacters) rejected with exit 2, formula untouched"

# --- Edge cases: empty string, trailing newline, "v" prefix left on ---
for bad in "" $'0.5.0\n' "v0.5.0"; do
  reset_formula
  set +e
  run_script "$bad" >/dev/null 2>&1
  STATUS=$?
  set -e
  [ "$STATUS" -eq 2 ] || fail "invalid version '$bad' did not exit 2 (got $STATUS)"
  [ "$(cat "$FORMULA")" = "sentinel: pre-existing content" ] || fail "formula file was modified by invalid version '$bad'"
done
echo "PASS: empty string, trailing newline, and unstripped 'v' prefix all rejected"

# --- Well-formed versions must still work ---
reset_formula
run_script "0.5.0" >/dev/null 2>&1
grep -q 'version "0.5.0"' "$FORMULA" || fail "valid version '0.5.0' was not embedded in the formula"
echo "PASS: valid version '0.5.0' accepted and embedded"

reset_formula
run_script "0.5.0-rc.1" >/dev/null 2>&1
grep -q 'version "0.5.0-rc.1"' "$FORMULA" || fail "valid pre-release version '0.5.0-rc.1' was not embedded in the formula"
echo "PASS: valid pre-release version '0.5.0-rc.1' accepted and embedded"

echo "All tests passed."
