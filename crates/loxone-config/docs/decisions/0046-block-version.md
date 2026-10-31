# D46: Managed blocks carry the document's block version

Status: accepted (2026-10-29)

## Context

Every `<C>` carries `V=`, a block-format version. Loxone Config writes every object at its release's version
on save, so each file has one (see [loxone-format.md](../loxone-format.md#objects-c)). Loxone Config migrates
an older block on load.

A rebuild that wrote a fixed `V="175"` was set back to 175 by every push into a 17.2 house, and stamped 178
again by every save. That was harmless, but no recompile of a saved config could reproduce it. Adopt refused
every block of a 17.2 file.

## Decision

- Compile emits the block version of the base's objects after the teardown (`LoxoneDoc::block_version`: the
  most common one, the higher on a tie), when `connectors::VERIFIED_BLOCK_VERSIONS` lists it.
- In a document of an unverified release, managed blocks get the first verified version (175), and Loxone
  Config migrates them on load. A block stamped with a version whose layout nobody checked could be read as
  something it is not.
- Adopt accepts a block of any listed version.
- A release is admitted after both oracle runs (mint and rebuild) pass under it. Then its version joins the
  list.

The release pin of [D22](0022-config-version-pin.md) is separate. `--accept-version` accepts a release; this
decision only sets the `V=` of managed blocks.

Validated for 178 (Loxone Config 17.2): all 24 builtin types minted at `V="178"` survive open+save with their
connector keys and indexes, and the house rebuild saves with an empty semantic diff. Oracle session 14, see
[oracle-sessions.md](../oracle-sessions.md).
