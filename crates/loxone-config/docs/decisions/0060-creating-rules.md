# D60: Creating Automatic Designer rules

Status: accepted (2026-10-30), amended by [D67](0067-deleting-rules.md)

## Context

[D57](0057-automatic-designer-rules.md) edits existing rules only. Loxone Config keeps a rule created outside
the app through open and save (see
[loxone-format.md](../loxone-format.md#automatic-designer-rules-c-typeautopilotrule-apc)), so the compiler can
create one.

## Decision

`rule <slug> "Name"` declares a rule by name, with the body of [D57](0057-automatic-designer-rules.md).

- **The name finds, the lock keeps.** The compile takes the base's one rule of that name; two rules of one
  name are refused. It pins the rule's UUID in the lock (v7 `rules`, slug → UUID), like pages
  ([D56](0056-creating-pages.md)). A base without that UUID gets the rule again under it.
- **The source owns the name.** Changing it rewrites `Title=` and the payload's `name` instead of creating a
  second rule. The lock keys by slug, not name, so a rename in source is a rename. `loxl rename` re-keys it.
- **Built from the base's own rules.** A new rule copies `V`, `Cl`, `Nio`, `WF` and `ref=` (the `AutoPilot`
  container) from the document's last rule. It goes on that rule's page, below the page's rules (gap 168, as
  in the house). Its seven connectors are minted right before it, as the Miniserver does, so their UUIDs
  follow from the rule's.
- **It starts inactive and empty.** The D57 edit then writes the body, so a created rule needs the same
  evidence (action `descID`s) as an edited one.
- **A config with no rule is refused.** Where rules live (a special page or logic pages) is the app's choice,
  and one rule made in the app settles it.
- **Removing the declaration** drops the lock entry and leaves the rule. `removed <slug>` deletes it
  ([D67](0067-deleting-rules.md)).
- A slug already declared otherwise is refused. So is a named rule that a `rule <extern>` also declares.
- **Taking rules over** (#192). Decompile and adopt declare a liftable rule by its name, with no extern, when
  no other rule shares the name. Name twins stay extern rules. `loxl adopt --uuid <rule>` does this for one
  rule of an existing project: it promotes the extern rule, and its `extern` line and unnamed `rule` go. The
  house's five app rules went over with no semantic change: 157 of 157 page blocks managed.

Validated: oracle session 24 (byte-identical except `WF=`, which the save drops from every rule; see
[oracle-sessions.md](../oracle-sessions.md)). Live on the house (firmware 17.2.8.28, 2026-10-30, #181): a
created rule, pushed with a verified byte-exact download, ran at its compiled minute and switched its target.
Pushed again as `inactive`, it stopped. The app's Automatic Designer listed it.
