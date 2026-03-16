class Hebbs < Formula
  desc "Cognitive memory engine — store, recall, reflect, and forget knowledge"
  homepage "https://hebbs.dev"
  version "0.3.0"
  license "BSL-1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.0/hebbs-macos-arm64.tar.gz"
      sha256 "d6f79cba1ca8d81eb1f2903649ca384a08e5b8bbf8b5f4cf9ecd9d477eed024f"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.0/hebbs-linux-x86_64.tar.gz"
      sha256 "50c727e67cd8dd35ce1372ed194725c2eec597211047e9528de730e345a81efe"
    elsif Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.0/hebbs-linux-aarch64.tar.gz"
      sha256 "11858223d63a1881f057954ace894ce6ca0ee4c2f004248e459d7d33beb03328"
    end
  end

  def install
    bin.install "hebbs"
    bin.install "hebbs-bench" if File.exist?("hebbs-bench")
  end

  test do
    assert_match "hebbs", shell_output("#{bin}/hebbs --version")
  end
end
