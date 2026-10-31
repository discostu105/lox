# D34: Mirror routing

Status: accepted (2026-08-25), amended by [D38](0038-unique-uuids.md)

## Context

With minted refs ([D33](0033-minted-mirrors.md)), consumer source no longer needs to name refs. A wire can name
the mirrored object, and the compiler draws it through a ref on the page. The GUI reads a mirrored signal through
the ref on the consumer's own page; a cross-page read without a local ref is a legal direct wire. Of the house
config's consumer wires, all 96 on `ref.AQ` and 5 on `ref.Q` are same-page.

## Decision

A wire that names the mirrored object reuses a ref the base already carries on the page. Routing is reuse-only
and page-local. All rules refuse rather than guess.

- **Input side.** `X: obj.port` routes through a base ref if the ref mirrors `obj`, is fed from exactly that port,
  and sits on the consuming block's page. The feed `obj.idx0 → AI` serves `AQ`; `obj.idx1 → I` serves `Q`.
- **Output side.** `obj.port <- y`, where the base feeds `obj.port` from an `OutputRef.AQ`, lands on that ref's
  `AI` on the writer's page. The `OutputRef.AQ` → actor distribution wire stays in source; the corpus draws all 154
  explicitly. API distribution wires are recorded in reverse
  ([loxone-format.md](../loxone-format.md#refs-c-typeinputrefoutputref-ref)), so the redirect also applies to a
  wire's source side.
- **Several candidates on the page.** The wire drawn before pins the choice. It is read from the base before the
  teardown, because the lock's `extern_wires` does not record wires into managed sinks. Without a pin, the compile
  refuses and names the candidates and the explicit-extern escape hatch. Every mirror has its own output ports
  ([D38](0038-unique-uuids.md)), so the choice matters.
- **Ref endpoints are drawn literally.** A wire that names a ref, a D32 extern or a D33 block, is not re-routed.
  This also keeps feed wires in place.
- **No fed ref on the page:** a direct wire.

Decompile and adopt invert the rules. A consumer wire drawn on a ref is emitted against the mirrored object when
the consumer shares the ref's page and the feeds are conventional: target idx0 → `AI`, idx1 → `I`, one
`OutputRef.AQ` sink. A ref is not lifted into the full view on its own. It appears only while an unfoldable wire
still names it, the way its plumbing wires fold ([D29](0029-full-and-adoptable-view.md)).

Validated on the house config: folding all 162 foldable ref externs out of the sources, onto 153 target externs,
recompiles byte-identical to the deployed config. 12 refs stay: cross-page consumers and source-fed export mirrors. The full view drops from
174 ref externs to 12.
