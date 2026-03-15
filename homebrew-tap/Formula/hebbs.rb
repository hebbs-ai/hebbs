class Hebbs < Formula
  desc "Cognitive memory engine: store, recall, reflect, and forget knowledge"
  homepage "https://hebbs.dev"
  version "0.2.0"
  license "BSL-1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.2.0/hebbs-macos-arm64.tar.gz"
      sha256 "f668e491d96c8f33da442c24251fca0ea24f8525873d543745ff8817c9954ff8"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.2.0/hebbs-linux-x86_64.tar.gz"
      sha256 "d20b064a8ef239dc67158d0f0d29274be46fca3dd5ad02791c34e1ba0d390317"
    elsif Hardware::CPU.arm?
      url "https://github.com/hebbs-ai/hebbs/releases/download/v0.2.0/hebbs-linux-aarch64.tar.gz"
      sha256 "48cadeb0a194dedafa0a0e470734800797008c0ffe670626f845aa2e7eb3d5a1"
    end
  end

  def install
    bin.install "hebbs"
    # Backward-compat symlinks (removed in v0.3.0)
    bin.install_symlink "hebbs" => "hebbs-cli"
    bin.install_symlink "hebbs" => "hebbs-vault"
    bin.install "hebbs-bench" if File.exist?("hebbs-bench")
  end

  def caveats
    <<~EOS
      HEBBS runs locally with zero configuration. No server needed.

      Quick start:
        hebbs init .
        hebbs remember "hello world" --format json
        hebbs recall "hello" --format json

      Data is stored in .hebbs/ (project) or ~/.hebbs/ (global fallback).

      Note: hebbs-cli and hebbs-vault are symlinks to hebbs for backward
      compatibility. They will be removed in v0.3.0.
    EOS
  end

  test do
    assert_match "hebbs", shell_output("#{bin}/hebbs --version")
  end
end
