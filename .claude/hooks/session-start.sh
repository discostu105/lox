#!/bin/bash
set -euo pipefail

# Only run in remote (Claude Code on the web) environments
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

# ── Rust toolchain ────────────────────────────────────────────────────────────
# Rust is pre-installed in the container, but ensure cargo is on PATH
if ! command -v cargo &>/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> "$CLAUDE_ENV_FILE"
fi

# Ensure clippy and rustfmt are available
rustup component add clippy rustfmt 2>/dev/null || true

# Build dependencies (cached after first run, subsequent sessions are fast)
cd "$CLAUDE_PROJECT_DIR"
cargo build 2>/dev/null || true
cargo test --no-run 2>/dev/null || true

# ── GitHub CLI (gh) ───────────────────────────────────────────────────────────
if ! command -v gh &>/dev/null && [ ! -f "$HOME/bin/gh" ]; then
  GH_VERSION="2.87.3"
  mkdir -p "$HOME/bin"
  curl -sL "https://github.com/cli/cli/releases/download/v${GH_VERSION}/gh_${GH_VERSION}_linux_amd64.tar.gz" \
    -o /tmp/gh.tar.gz
  tar -xzf /tmp/gh.tar.gz -C /tmp
  cp "/tmp/gh_${GH_VERSION}_linux_amd64/bin/gh" "$HOME/bin/gh"
  rm -rf /tmp/gh.tar.gz "/tmp/gh_${GH_VERSION}_linux_amd64"
fi

# Ensure ~/bin is on PATH for gh
echo 'export PATH="$HOME/bin:$PATH"' >> "$CLAUDE_ENV_FILE"
