# D40: Program code from source; numeric character references are text

Status: accepted (2026-10-29)

## Context

Loxone does not decode numeric character references. `&#xA;` in an attribute is five characters of text, and a
GUI save re-escapes the ampersand ([loxone-format.md](../loxone-format.md#not-actually-xml)).

A foreign XML writer pushed the house config with a PicoC program's line breaks encoded as `&#xA;`. That program
is the Code16 block that drives the Hue lights. After the next GUI save, the 503-line program was one line starting
with `//`: a single comment. The same save damaged two `AutopilotRule` `APC=` payloads (now invalid JSON) and ten
Modbus `NDOC=` texts.

Lox Logic could neither see nor repair this. `Code=` was GUI residue outside the diff
([D19](0019-gui-residue-carried-forward.md)), and `xml::unescape` decoded numeric references, so Lox Logic read
those attributes differently from Loxone.

## Decision

- **Numeric character references are text.** `xml::unescape` decodes only the five named entities and keeps
  `&#…;` verbatim, which is what Loxone reads. No config in the 133-file corpus contains one, so no GUI-written
  content reads differently. `loxl drift` starts with a note listing every attribute that holds one
  (`LoxoneDoc::char_ref_attrs`), program code and `APC=` first. `loxl repair` restores them
  ([D44](0044-loxl-repair.md)).
- **`Code: file("prog.c")`** on Code16 is an attribute parameter, like `Formula:` ([D18](0018-adoption.md)).
  - A PicoC program is hundreds of lines, too long for the line-oriented language, so the value comes from a file.
  - The path is relative to the `.loxl` file that names it. It is read when the module loads
    (`Module::load_files`).
  - CRLF is normalized to LF. The text is written the way Loxone Config writes it: raw LF line breaks and the five
    entities.
  - `file(…)` binds only file parameters (`connectors::file_params`, so far Code16 `Code`). Anywhere else it is an
    error. `Task=` stays GUI-owned.
- **A GUI-owned attribute parameter is a claim.** `Code` stays in the carried set
  ([D19](0019-gui-residue-carried-forward.md)).
  - Unclaimed, it is carried verbatim from the base, damage included.
  - Claimed, the source text replaces the carried copy.
  - A withdrawn claim hands the program back to the GUI. It needs no tombstone, because the base holds the last
    pushed text.
- **Decompile never lifts `Code`.** A decompiled module writes no files, and a program inlined into the view would
  be unreadable. Adoption leaves the program with the GUI until an author moves it into a file. Adopt accepts the
  `Valid=` companion only on `Formula`.
- **The diff and the fingerprint see program text.** A `Code` change is a parameter change, printed as a line diff
  (`diff::text_changes`: common prefix and suffix trimmed, at most eight lines per side). Fingerprint v3 added
  program text; `loxl drift` notes an older fingerprint that cannot see it. The lockfile format is unchanged.

Validated on the house config: compiling with the program in `philipps_hue_steuerung.c` reproduces the
Loxone-written `Code=` from before the damage byte for byte. Every byte outside that attribute equals the base, and
the semantic diff shows one change: the program going from 1 line back to 503.
