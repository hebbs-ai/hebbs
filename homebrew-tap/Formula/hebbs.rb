class Hebbs < Formula
  desc "Cognitive memory engine — store, recall, reflect, and forget knowledge"
  homepage "https://hebbs.dev"
  version "0.3.4"
  license "BSL-1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-macos-arm64.tar.gz"
      sha256 "d0fc9466c64324d40b652dcd6bd3902a48fb4c599ef0bf6eec9a832d74776ed5"
    elsif Hardware::CPU.intel?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-macos-x86_64.tar.gz"
      sha256 "fb4589701d7fcb6f18133a7052956783e5f9acdd570bd0791420d4bbc510a8bb"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-linux-x86_64.tar.gz"
      sha256 "4c574858d7a160ddc61fd2ce881bda7f62d7683cc4c9dd7f7a03a95697b3abc4"
    elsif Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.4/hebbs-linux-aarch64.tar.gz"
      sha256 "85784c9382ebef070065a19b6686e1e5ce2d2a31e80b31553b11f6f53700a1d9"
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
