class Fini < Formula
  desc "A lightweight file normalization CLI tool for AI coding agents"
  homepage "https://github.com/tsukasaI/fini"
  version "0.4.0"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-apple-darwin.tar.gz"
      sha256 "106c0d0ebfbf29f2fbb730c375f3f70fe37ba0c6e746d8b25b324e113660ba72"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-apple-darwin.tar.gz"
      sha256 "f30a6661857bf255deaf4c39d7c2e55af47359b947d9137ac5b787a8343a1cd1"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "6c9b6affe64353cdf2bed3d5e36f72b08b1d2a71f4b8fbf090b0e8d65ed4e4e4"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "83d544413344d26ae96855a389fa2d6e1a46c5ecfac9bc9e01f11dc4c28d8ca7"
    end
  end

  def install
    bin.install "fini"
  end

  test do
    system "#{bin}/fini", "--version"
  end
end
