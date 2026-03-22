class Lox < Formula
  desc "Loxone Miniserver CLI — control lights, blinds, and automations from your terminal"
  homepage "https://github.com/discostu105/lox"
  version "0.10.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-aarch64"
      sha256 "e74d9a3d5218d66369e841fc4e4695edaa3ef5a6cfffe937b60340d7eb0d19bd"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-x86_64"
      sha256 "4e36315ebce42d58f8b90b7496e126eaf66e0e4da33d349fdfb17e4d368fb464"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-aarch64"
      sha256 "7aa8a2fe05c259d1f3c0171ba8116625ffb5276a965791ba7557802c64f973eb"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-x86_64"
      sha256 "c0c53d7ab7e95b302ea2afee095b706c36c4943fec4b7dd5313b05e4abc1b254"
    end
  end

  def install
    binary = Dir.glob("lox-*").first || "lox"
    chmod 0755, binary
    bin.install binary => "lox"

    generate_completions_from_executable(bin/"lox", "completions")
  end

  test do
    assert_match "lox", shell_output("#{bin}/lox --help")
  end
end
