# Contributing to lox

## Development Setup

```bash
# Clone
git clone https://github.com/discostu105/lox
cd lox

# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run tests (no Miniserver required)
cargo test

# Lint
cargo clippy -- -D warnings
cargo fmt --check
```

No live Miniserver is needed for development. All unit tests use pure functions
or mocked data. Integration tests (if added) use `wiremock`/`httpmock`.

## Issue Tracking

Bugs and feature requests are tracked in [GitHub Issues](https://github.com/discostu105/lox/issues).

## Manual Testing

To test against a real Miniserver you need:

- A Loxone Miniserver (Gen 1 or Gen 2) on your local network
- Host, username, and password in `~/.lox/config.yaml` (run `lox config set`)

```bash
lox ls                   # list all controls
lox status               # Miniserver health
lox token fetch          # acquire auth token
```

## PR Process

- One feature or fix per PR
- Tests expected for new logic (pure functions are easy to unit-test)
- Run `cargo fmt` and `cargo clippy` before submitting
- Update `CHANGELOG.md` under `[Unreleased]`

## Architecture

See [`CLAUDE.md`](CLAUDE.md) for a full architectural overview of the source files.
