# Contributing to lox

The full guide is [docs/contributing.md](docs/contributing.md): setup, build commands, references and manual
testing. [AGENTS.md](AGENTS.md) describes the architecture and the release flow.

## Quick start

```bash
git clone https://github.com/discostu105/lox
cd lox
cargo build
cargo test          # no Miniserver needed
```

## Before you push

All four checks must pass. They mirror CI:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo build --release
cargo test
```

## Pull requests

- One feature or fix per PR, with tests for new logic.
- Add a changelog entry as a new file in [`changelog.d/`](changelog.d/README.md). Don't edit `CHANGELOG.md`.
- Merged PRs are released automatically. Label the PR `release:major`, `release:minor`, `release:patch` or
  `no-release` to steer the version bump. The default is minor with an `added` entry, patch otherwise.
- Contributions are licensed under the project's [Apache-2.0 license](LICENSE).

Bugs and feature requests live in [GitHub Issues](https://github.com/discostu105/lox/issues). Report security
problems as described in [SECURITY.md](SECURITY.md).
