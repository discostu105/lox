# D133: Externs take bindings, expressions wrap and compute, `page` is a file header

Status: accepted (2026-11-03), language v3 part 2 (#374)

## Context

v2 made "port P of X is fed by Y" depend on who owns X: `P: Y` in the argument list of a managed block,
`X.P <- Y` for an extern, and an error for each spelling in the other place. The `extern` keyword
already says who owns the object. Fan-in was a repeated key (`In1: a, In1: b`), which every other
language reads as a mistake. A long `and` chain could not wrap, so the rules that matter most were
the least readable lines. Arithmetic was allowed only as a whole right-hand side, although the
`Formula` block evaluates comparisons itself. `page` was positional: it governed the statements
below it to the next `page`, there was no way back to the default page, and a block above a file's
first `page` silently kept the default page.

## Decision

**Externs take port bindings.** `extern jal = AutoJalousie(title: "Süd", AutoShade: beschatten,
TargetPos: 70)` means what the same bindings mean on a block. Matcher keys are lowercase, ports
PascalCase, so the two never collide. Before the checks and the compile, the bindings lower to the
`<-` and `=` statements (`Module::lowered`), so the lockfile keeps tracking extern wires and the
parameter originals exactly as before. Decompile and adopt write the bindings and fold the
statement forms they used to write.

**`<-` and `=` work on every sink.** They are the statement forms of a binding, kept for a binding
written in another file than the declaration. On a block or instance the module declares they fold
into its argument list on expansion (`fold_remote_bindings`), so they compile exactly like the
argument, and `loxl lint` reports them as `remote-binding`: the declaration is where a reader looks.
The v2 errors for a managed sink are gone.

**Fan-in is a list.** `In1: [a, b]` parses into one binding per source. A repeated key still parses
and means the same; `fmt` writes the list wherever a port has several sources without comments, so
decompiled fan-ins read as lists.

**Expressions wrap.** The parser joins a line with the next when the line ends with an operator or
`<-`, when the next line starts with `and` or `or`, or while an expression group's parenthesis (one
that follows no name) is open. A comment goes on the last line. `fmt` breaks a top-level `and`/`or`
chain that runs past 100 columns before each operator, one tab deeper than the statement.

**Arithmetic under a comparison and under a gate compiles.** `x.AQ + 1 >= 5` is one `Formula` block
whose text holds the comparison (`I1+1>=5`), since the block evaluates it and yields 1 or 0; both
sides may hold arithmetic. Under a gate (`a and x.AQ + 1`) the arithmetic is its own `Formula`
whose `AQ` feeds the gate, a wire like any other. The simulator already evaluated both. A boolean
group still cannot enter arithmetic or a comparison.

**`page` is the file's header.** At most one per file, before every other statement; comments may
precede it. It governs the whole file. A `page` below a statement or a second one is a parse error
that says to move the line or to split the file. A config with blocks on several pages is several
files: `adopt --out-module` refuses it, `decompile` without `--out-dir` prints the files `--out-dir`
would write under `# ---- <file>.loxl` lines, and a one-page config is one file with its page line
first, the periphery after it. The examples and the test fixtures with two pages in one text became
directories or fragments.

## Rejected

- **Dropping `<-` and `=` altogether.** A wire onto an extern declared in another file (the
  periphery file) would then need the extern repeated or moved. The statement form stays, with the
  lint pointing at the declaration.
- **A repeated key as an error.** Decompiled v2 modules and the lockfile-neutral migration would
  need a parse-time rewrite; `fmt` canonicalizing it is the rule v2 set for every other second
  spelling.
- **A `page` anywhere in the file, governing the whole file.** A v2 file with blocks above its `page`
  line would silently move them. The header rule turns that into an error that names the fix.
- **Joining lines on indentation.** Tabs carry no meaning anywhere else in the language.
