# D14: Port assignment is for extern ports only

Status: accepted (2026-08-25), amended by [D16](0016-constructor-syntax.md)

## Context

A write to an extern port is tracked: the compiler records the old value and restores it when the line goes
away. A parameter of a managed block is plain. One keyword with two ownership meanings, and two spellings for
a block parameter, confuse both reader and validator.

## Decision

Port assignment `target.Port = value` always means a tracked write to somebody else's port. A managed block's
parameters live in its argument list ([D16](0016-constructor-syntax.md)). The validator points an assignment
to a managed block at the argument list.
