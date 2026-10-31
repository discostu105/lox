# D45: A rebuilt wire keeps its place among the sink's `<In>`

Status: accepted (2026-10-29)

## Context

Loxone Config writes a connector's `<In>` children in an order of its own when it saves
([loxone-format.md](../loxone-format.md#connectors-and-wires-co--in)). The order means nothing. But the teardown
removes the compiler's wires and the rebuild appended them, so recompiling a GUI-saved config moved compiler wires
to the end of their connector. The bytes changed with the logic unchanged, and `lox config push` offered an upload,
with its reboot, for nothing.

## Decision

Before the teardown, the compile records each sink's `<In>` sources in base order. A wire added back goes in before
the first `<In>` that the base listed after it, or that the base did not have. So it lands where the base had it.

- **Managed and extern sinks alike.** A managed block is rebuilt empty and gets its wires back in base order. On an
  extern sink, the GUI's wires stay put and the compiler's wires return between them.
- **New wires append.** A wire the base did not carry has no place to keep and goes last.
- **An `<In>` already present stays.** The first compile after adopt finds the declared wire in the base and leaves
  it where it is. Every later compile puts it back there.

Compiling the compiler's own output is still a fixpoint, since that output is a base like any other.

The rest of what a save reorders or adds is [D46](0046-block-version.md) (block version),
[D47](0047-block-order-kept.md) (block, attribute and connector order) and [D48](0048-text-ref-flag.md)
(`Text="true"` on refs). With all four, compiling a config that Loxone Config 17.2 saved reproduces it byte for byte.
