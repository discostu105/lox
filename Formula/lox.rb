class Lox < Formula
  desc "Loxone Miniserver CLI — control lights, blinds, and automations from your terminal"
  homepage "https://github.com/discostu105/lox"
  version "0.45.0"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-aarch64"
      sha256 "6c99a8ecb5c307337341ec0aa89be68c18fdcaf93ca9fd2ea08b4fce632ea1bf"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-x86_64"
      sha256 "32aaa38cd3e5196dc24ad1c8b307f0e6a23b9478d138e0faf41dca7eadae96a2"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-aarch64"
      sha256 "72125502cbc4468b3026a3b71286e3ffcb537ba68a10f34c1974123271892e78"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-x86_64"
      sha256 "912714b4438727ac0a15b0c23e6d2a097e583517a654632d783def600b7137be"
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
