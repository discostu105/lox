# Loxcall (Smarteon)

Call-state handling for Smarteon's Loxcall SIP integration: a Program block parses
the call state text from a virtual text input into memories (state, user, device)
and comparators that raise "start" and "ringing". Saved by Loxone Config 12.1
(`ConfigVersion 12010716`).

- Source: https://github.com/Smarteon/community/blob/a2d76ac61783d3f4851367143d7f2f15cc528f20/loxcall/loxcall.Loxone
- Commit: `a2d76ac61783d3f4851367143d7f2f15cc528f20` (sha256 of the original:
  `ac2521613fa000fa88453e793bbeed7551b9f95b8e803056b898da412c9cfa3b`)
- Author: Smarteon Systems s.r.o.
- License: BSD-3-Clause, see [LICENSE](LICENSE)
- Changes: the line `<!-- Normalized by Smarteon Systems -->` removed (Loxone
  Config never writes comments, and the parser refuses them), then
  placeholders for 9 values with `loxl scan --redact`: the document's creator,
  phone, mail address and coordinates, two user names, the Miniserver's serial
  and remote-access address. Nothing else.

Files: [loxcall.Loxone](loxcall.Loxone) (the edited config),
[logic/loxcall.loxl](logic/loxcall.loxl) (its Lox Logic view),
[loxcall.svg](loxcall.svg) (the page as drawn).

Worth a look: the comparator `START` has a wire on `Input1` and still keeps a
constant (`Def="2"`) there from before the wire was drawn; the view prints both.
