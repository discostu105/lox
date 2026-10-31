# D44: `loxl repair` turns numeric character references back into characters

Status: accepted (2026-10-29)

## Context

A foreign XML writer encoded line breaks as `&#xA;`, which Loxone reads as text
([loxone-format.md](../loxone-format.md#not-actually-xml)). Lox Logic reads them the same way and `loxl drift` lists
the attributes that hold them ([D40](0040-program-code.md)). The house's two damaged `APC=` payloads had been
byte-patched by hand, and ten Modbus `NDOC=` texts still showed the literal `&#xD;&#xA;`.

## Decision

`loxl repair <cfg> --out <out>` writes a repaired copy, using `LoxoneDoc::repair_char_refs`.

- **It writes what Loxone wrote before the damage.** Each reference becomes the character it names, written the way
  Loxone writes it: line breaks and every other character raw, `& < > " '` as the five named entities. A foreign
  writer encodes characters one by one, so decoding them one by one restores the original. A `&#xD;&#xA;` pair
  becomes CRLF, a lone `&#xA;` a bare LF.
- **Both damaged forms are repaired.** `&amp;#xA;` (re-escaped by a later GUI save) and `&#xA;` (from the foreign
  writer, not yet saved by the GUI) both read as the text `&#xA;`.
- **A reference that names nothing an attribute can hold** (`&#0;`, a surrogate) stays text, and the command names
  what it left.
- **Every other byte is copied.** The rewrite works on the raw attribute value and touches only the references.
  The scope is that of `char_ref_attrs`: attributes of `<C>` elements.
- **A copy, not in place.** The input stays as evidence. Each rewritten attribute is listed with its object, UUID
  and reference count.
- **No compile step.** The repaired file is a base like a download: compile against it and push the output. The
  repaired attributes are GUI content (`NDOC`, `APC`, an unclaimed `Code`), so they reach the Miniserver through the
  compiled output, and drift sees the push.

Validated: oracle session 13 ([oracle-sessions.md](../oracle-sessions.md)). Repaired corpus configs equal their
undamaged originals byte for byte, and Loxone Config keeps repaired `APC=` and `Code=` byte-identical on open and
save. On the house config the ten `NDOC=` texts come back with raw CRLF and the save keeps them.
