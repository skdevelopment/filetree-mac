class Filetree < Formula
  desc "TreeSize-style disk usage analyzer for macOS"
  homepage "https://github.com/skdevelopment/filetree-mac"
  url "https://github.com/skdevelopment/filetree-mac/archive/refs/tags/v0.3.0.tar.gz"
  sha256 "a522699695a653142c5f928fa44d3a7fe2ed180eed1a23189d616e90171f25c3"
  license "MIT"
  head "https://github.com/skdevelopment/filetree-mac.git", branch: "main"

  depends_on :macos
  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--locked"
    # macOS endpoint security can SIGKILL a Mach-O named exactly "filetree";
    # ship filetree-mac and a thin wrapper (same pattern as install.sh).
    bin.install "target/release/filetree-mac"
    (bin/"filetree").write <<~EOS
      #!/bin/bash
      exec "#{bin}/filetree-mac" "$@"
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/filetree-mac --version")
    assert_path_exists bin/"filetree"
  end
end