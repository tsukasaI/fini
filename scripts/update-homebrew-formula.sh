#!/usr/bin/env bash
# Regenerate HomebrewFormula/fini.rb for a release.
#
# Usage: scripts/update-homebrew-formula.sh <version> <assets-dir>
#   <version>    release version without the leading "v" (e.g. 0.4.0)
#   <assets-dir> directory containing the four released fini-<target>.tar.gz
#                files and checksums.txt for that version
#
# Verifies the tarballs against checksums.txt before computing anything, so a
# corrupt or tampered download can never end up hashed into the formula.
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <version> <assets-dir>" >&2
  exit 2
fi

VERSION="$1"
ASSETS_DIR="$2"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FORMULA="$REPO_ROOT/HomebrewFormula/fini.rb"

if command -v sha256sum >/dev/null 2>&1; then
  SHA256() { sha256sum "$@"; }
else
  SHA256() { shasum -a 256 "$@"; }
fi

(cd "$ASSETS_DIR" && SHA256 -c checksums.txt >&2)

sha_for() {
  SHA256 "$ASSETS_DIR/fini-$1.tar.gz" | awk '{print $1}'
}

SHA_MAC_INTEL="$(sha_for x86_64-apple-darwin)"
SHA_MAC_ARM="$(sha_for aarch64-apple-darwin)"
SHA_LINUX_INTEL="$(sha_for x86_64-unknown-linux-gnu)"
SHA_LINUX_ARM="$(sha_for aarch64-unknown-linux-gnu)"

cat > "$FORMULA" <<EOF
class Fini < Formula
  desc "A lightweight file normalization CLI tool for AI coding agents"
  homepage "https://github.com/tsukasaI/fini"
  version "$VERSION"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-apple-darwin.tar.gz"
      sha256 "$SHA_MAC_INTEL"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-apple-darwin.tar.gz"
      sha256 "$SHA_MAC_ARM"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$SHA_LINUX_INTEL"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "$SHA_LINUX_ARM"
    end
  end

  def install
    bin.install "fini"
  end

  test do
    system "#{bin}/fini", "--version"
  end
end
EOF

echo "Regenerated $FORMULA for v$VERSION" >&2
