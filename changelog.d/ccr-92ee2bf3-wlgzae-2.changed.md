- **Lox Logic v3, part 2: externs take port bindings.** `extern jal = AutoJalousie(title: "Süd",
  AutoShade: beschatten, TargetPos: 70)` wires and sets like the same bindings on a block. `<-` and
  `=` work on every sink; on a block the module declares they fold into its argument list and
  `loxl lint` reports `remote-binding`.
- **Lox Logic: fan-in is a list.** `In1: [a, b]` puts several sources on one input; `fmt` writes it
  for a repeated key.
- **Lox Logic: expressions wrap and compute.** A long `and`/`or` chain continues on the next line,
  and `fmt` breaks one past 100 columns. `x.AQ + 1 >= 5` compiles to one `Formula` block,
  `a and x.AQ + 1` feeds the gate from one.
- **Lox Logic: `page` is the file's header.** At most one per file, before every other statement. A
  file with two pages becomes two files; `decompile` prints several pages as several files, and
  `adopt --out-module` refuses a config with blocks on several pages.
