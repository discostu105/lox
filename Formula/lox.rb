class Lox < Formula
  desc "Loxone Miniserver CLI — control lights, blinds, and automations from your terminal"
  homepage "https://github.com/discostu105/lox"
  version "0.1.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-aarch64"
      sha256 "1dd73b2d607a99c577295ced3638edfadc4e73eae77a4953ff487302acb952c4"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-x86_64"
      sha256 "c35aee4d6afa98ae7e0e7ea4242a70bccec9d38909d28943fe6f7cc1cd105401"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-aarch64"
      sha256 "039245ed681bbd5c2ec1d81216e53fbd0900af3f8f8af77bb6572be052567bda"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-x86_64"
      sha256 "aa5a05eb7c9f6a6e7f3ae01c60e9abc30a9935687fab638adcd905e0084f3d79"
    end
  end

  def install
    binary = Dir.glob("lox-*").first || "lox"
    bin.install binary => "lox"
  end

  test do
    assert_match "lox", shell_output("#{bin}/lox --help")
  end
end
