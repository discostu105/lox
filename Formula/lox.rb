class Lox < Formula
  desc "Loxone Miniserver CLI — control lights, blinds, and automations from your terminal"
  homepage "https://github.com/discostu105/lox"
  version "0.28.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-aarch64"
      sha256 "a0e243c6cbefd382555ad7c15a0ff827ee2a4b4d8f321b2696f8c10f7f6b22a3"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-x86_64"
      sha256 "11a00d915e8d113442c8bb7efa35803c3f1fff47989d232ea7e34cb7ae949953"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-aarch64"
      sha256 "ac7151bda3aef1ef19ce12d602a740bdf942bffb7a2725a2522a296272a8c330"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-x86_64"
      sha256 "fd67eca10af43ad3c2a8d2ed0cbb1dc3e484a7cd7e95173e357a8d3160c01cbc"
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
