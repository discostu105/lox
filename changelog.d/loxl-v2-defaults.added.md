- **`loxl` / `lox logic`: `defaults <Type>(Port: value, …)`** gives every managed block of a type the
  values it does not bind itself, template bodies included. It compiles to the same bytes as writing
  the values on each block
  - Values only, one statement per type, at the top level. `loxl lint` reports one that no block takes
    (`unused-default`), and hovering a block in the editor lists the values it takes
  - `loxl decompile` and `loxl adopt` write it for a type whose blocks sit on one page and mostly
    share a value. They also write time values with a unit (`Delay: 3h`, `Time: 500ms`) and keep the
    wires into one extern together
