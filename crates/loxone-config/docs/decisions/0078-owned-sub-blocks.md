# D78: A block's own blocks stay with it

Status: accepted (2026-10-31)

## Context

Some blocks hold `<C>` children that Loxone Config creates with them. An energy flow monitor holds a nested
`MeterAbsBi` *Rest* for the unmetered consumption (see
[loxone-format.md](../loxone-format.md#objects-c)). A Code Touch holds its `Keycode`s. The compile
carries these children verbatim with their block ([D19](0019-gui-residue-carried-forward.md)). Decompile
lifted such a child as a block of its own (`rest = MeterAbsBi()`), and an adoption claimed it in the lock.
Issue: #236.

## Decision

A `<C>` nested in a block whose child `<C>`s are carried (`carried_children`) belongs to that block.

- The view leaves it out.
- An adoption leaves it to its owner.
- `loxl adopt` of its UUID is refused, with a pointer to the owning block.
