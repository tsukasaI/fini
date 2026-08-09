class Fini < Formula
  desc "A lightweight file normalization CLI tool for AI coding agents"
  homepage "https://github.com/tsukasaI/fini"
  version "0.5.0"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-apple-darwin.tar.gz"
      sha256 "c815dd5a5a03bbf7e6b9767fff345c3cde6bcd28e7053184a88e777d304c7ebb"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-apple-darwin.tar.gz"
      sha256 "de4b0b88cd78c4c3e7078c59aa12260d0fa9a91e9929ab9175a1049c88bdecfc"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "3945e068c7b1a484e5cdfd735e37b2f643deec7b96d8953333a40fb96f5aaaa2"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "d6f3367088aaf5fb477c4969a475b8063147dbb67e58806f7b40fbc34b304371"
    end
  end

  def install
    bin.install "fini"
  end

  test do
    system "#{bin}/fini", "--version"
  end
end
