class Hebbs < Formula
  desc "Cognitive memory engine — store, recall, reflect, and forget knowledge"
  homepage "https://hebbs.dev"
  version "0.3.2"
  license "BSL-1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.2/hebbs-macos-arm64.tar.gz"
      sha256 "6ead83091dc3fb54e9d7689546bd56a0b5c1dfef3691ebd71908ab8611132d74"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.2/hebbs-linux-x86_64.tar.gz"
      sha256 "cb73291b01c3ed4b40f1954620c27bc1320a48716563699b1e58443d789f9550"
    elsif Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.3.2/hebbs-linux-aarch64.tar.gz"
      sha256 "122c54e63b10b4ae4a633880ab51fd2938467fb5c16bd0b73bb1934d9e49cadb"
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
