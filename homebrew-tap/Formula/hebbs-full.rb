class HebbsFull < Formula
  desc "Cognitive memory engine (full): local vaults, indexing, serve, reflect"
  homepage "https://hebbs.dev"
  version "0.3.4"
  license "BSL-1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-macos-arm64-full.tar.gz"
      sha256 "PLACEHOLDER"
    elsif Hardware::CPU.intel?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-macos-x86_64-full.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-linux-x86_64-full.tar.gz"
      sha256 "PLACEHOLDER"
    elsif Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-linux-aarch64-full.tar.gz"
      sha256 "PLACEHOLDER"
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
