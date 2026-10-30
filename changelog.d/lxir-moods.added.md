- **Lighting moods from source** (`lox ir`/`lxir`): `moods <set> … end` lists a Lichtsteuerung's moods by name
  with the values of its circuits, and `<controller>.moods = <set>` makes them that controller's mood list
  - Values follow each circuit's kind: `on`/`off`, `40%`, `rgb(100, 80, 0)`, `temp(60%, 2700K)`; a value the
    circuit cannot take is a compile error
  - Claim a set inside a room template and every room gets the same moods. Existing moods keep their identity
    by name, so switch buttons wired to `Sel1`…`Sel8` keep selecting them
  - A controller minted from source gets Loxone Config's stock moods ("Viel Licht", "Aus") first, so it no
    longer needs a save in Loxone Config before its moods can be set
  - `lxir diff` shows mood changes per mood, and `lxir drift` reports moods edited in Loxone Config or the app

```text
moods wohnen
	"Viel Licht": all on
	"Lesen": AQ1 80%, AQ3 temp(60%, 2700K)
	"Kino": AQ4 rgb(0, 0, 20)
end
wohnzimmer_licht.moods = wohnen
```
