class Lox < Formula
  desc "Loxone Miniserver CLI — control lights, blinds, and automations from your terminal"
  homepage "https://github.com/discostu105/lox"
  version "0.56.1"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-aarch64"
      sha256 "5ac05bf20b9c1c4cde5cdd512ddd509fe24665c62ffa4e588d567d6f63df0349"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-x86_64"
      sha256 "012e0c3737c07eb5c8d73af89e446f220586896322c1f5ccb7b439de0ada4610"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-aarch64"
      sha256 "a0e4546fa579b9b88217cbb9b026561bf7db2a99e8967780f05229d3bd39848c"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-x86_64"
      sha256 "e6943b6ad56d7946fa362a27c300335af24c73faa660e36aaea9f36c4d32c29b"
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
