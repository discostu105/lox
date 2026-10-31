# D22: The Loxone Config release is a qualification pin

Status: accepted (2026-08-25), amended by [D46](0046-block-version.md)

## Context

Each Loxone Config release may change block descriptors, schema migrations and save behavior. The builtin
table and the residue model are validated against specific releases, with oracle runs (see
[oracle-sessions.md](../oracle-sessions.md)).

## Decision

The lock's `target.config_version` is enforced. Compiling against a base written by another release is a
hard error until that release is qualified:

1. One oracle open and save of a rebuilt config under the new release.
2. `loxl compile --accept-version <v>`, where `<v>` equals the base's version exactly.

Acceptance is per compile and deliberate. The lock then pins the new version. Adoption pins the release that
wrote the adopted config, so the first compile against the same file always passes.
[D46](0046-block-version.md) decides which block version `V=` the managed blocks get.
