class Lox < Formula
  desc "Loxone Miniserver CLI — control lights, blinds, and automations from your terminal"
  homepage "https://github.com/discostu105/lox"
  version "0.17.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-aarch64"
      sha256 "ec8d22e4254d5f7046a65b716976d482818289c15971f080d6f4d75e0a4ef388"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-x86_64"
      sha256 "464a2cdda117e5038cc552b3f566c81f58aa4488b328efe21c03b911a00c19d0"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-aarch64"
      sha256 "44fdc70a5e8aa19d67ae5395b204365a203602d7ee4c2697a0c6ecc49d5f9779"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-x86_64"
      sha256 "8a92116624eac8e3e56994e961313460e54c9709da17e72a4126e199ee6491fc"
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
