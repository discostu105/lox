- **Lox Logic v3, part 3: templates are block types.** A template is named like a type and called
  like a block (`sued = Fassade("Süd", jalousie: jal_sued)`). Its header declares the outputs
  (`-> (Q: beschatten.Q)`, read as `sued.Q`), its value parameters their type (`pos: number = 70`,
  `nachlauf: duration`), and an input parameter (`sonne: input`) is how a call feeds a port of the
  body. The primary block and call-site port bindings are gone; `loxl fmt --write` migrates a v2
  template. New: the label string, nested instances, body-local `let`, `loxl types <Template>`.
