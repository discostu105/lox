- **loxl**: a named signal takes a label of its own: `beschatten.title = "Beschatten Süd"` on the line
  below it sets the top block's title, which is otherwise the expression text (#288).
- **loxl decompile**, **loxl adopt**: the blocks an expression or named signal compiled to come back as
  that expression, signal and `let`, where they compile to the same blocks. Adopt marks them
  expression-owned in the lock.
