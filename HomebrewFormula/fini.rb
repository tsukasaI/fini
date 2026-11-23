class Fini < Formula
  desc "A lightweight file normalization CLI tool for AI coding agents"
  homepage "https://github.com/tsukasaI/fini"
  version "0.8.0"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-apple-darwin.tar.gz"
      sha256 "d1ea3c3cc84fae85252788cdb7ebe3b0a9fe256b41e42bcccddc1de5d6a95bbe"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-apple-darwin.tar.gz"
      sha256 "f23f6e822b834d761cb10e00dd735282fb5f734202f795193ad1a80c00cf601c"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "6f2edc6cfbfdf9bcbe76b1b289871a581e2dc289f41f23fd7725f631b4750f35"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "635b9ce9af7526abb882d0bb4397ce29f346ffff2c68589c268fab4382cdfb1c"
    end
  end

  def install
    bin.install "fini"
  end

  test do
    system "#{bin}/fini", "--version"
  end
end
