class Lox < Formula
  desc "Loxone Miniserver CLI — control lights, blinds, and automations from your terminal"
  homepage "https://github.com/discostu105/lox"
  version "0.6.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-aarch64"
      sha256 "fe58eaa7c887ed0348b1276a83cc3ed6d5d9f46d691e5adfdf64876cdfc0aebe"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-macos-x86_64"
      sha256 "37a63174275a5d231e81adf54b5b05213a2823cb77efe0d80580a6c6fa3b5e2a"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-aarch64"
      sha256 "01ba7e0eb8f7b8e1aff5f8aa4e24ffa76c5bc7c4bb17113a3ba3322735e9d537"
    else
      url "https://github.com/discostu105/lox/releases/download/v#{version}/lox-linux-x86_64"
      sha256 "9c5ef36072982286866bf7361f7819d15c07e764c33776d4ad5057e4530a9f59"
    end
  end

  def install
    binary = Dir.glob("lox-*").first || "lox"
    bin.install binary => "lox"

    generate_completions_from_executable(bin/"lox", "completions")
  end

  test do
    assert_match "lox", shell_output("#{bin}/lox --help")
  end
end
