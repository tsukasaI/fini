class Fini < Formula
  desc "A lightweight file normalization CLI tool for AI coding agents"
  homepage "https://github.com/tsukasaI/fini"
  version "0.3.0"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-apple-darwin.tar.gz"
      sha256 "64204d99f8939d0d71016f3b708b0ac7a86cc699d6a686f188376c1556919fe8"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-apple-darwin.tar.gz"
      sha256 "b30eded5d05aebb298b83a50cf4f37344f48cf0f7d19301c1b5c94f379bf42be"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "44ac7936f2c02a2129c93da878e3f7dd0a3a3deab123be8254badbad5b3ed70e"
    end
    on_arm do
      url "https://github.com/tsukasaI/fini/releases/download/v#{version}/fini-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "2bb25907627eb1b889183c677865efec6d27c443cc1205996681de15672123db"
    end
  end

  def install
    bin.install "fini"
  end

  test do
    system "#{bin}/fini", "--version"
  end
end
