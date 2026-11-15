# D136: Rooms and categories from source

Status: accepted (2026-11-15), reverses a non-goal of [vision.md](../vision.md) (#324)

## Context

A block could name its room and category in source ([D39](0039-attribute-claims.md),
[D63](0063-managed-room-category.md)), but only by title, and the room had to exist in the base. Setting
up a room end to end, an agent or a user still opened Loxone Config to create it. The vision listed rooms
and categories as staying in Loxone Config; #324 asked to reconsider.

A room is a `<C Type="Place">` under the `PlaceCaption` folder, a category a `<C Type="Category">` under
`CategoryCaption`. Oracle session 95 created both in Loxone Config 17.3 and found two more objects per
room: a permission group (`<C Type="RightGroup" GT="1">`, a category's `GT="2"`) in the permission groups
folder, which the room names in `RGR=`, and a sort node (`<nd t="1" ctrlUuid=…>`) in the room folder.
The app catalogue lists both under `rooms` and `cats`.

## Decision

**Rooms and categories are declared like operating modes** ([D69](0069-operating-modes.md)): an object of
the object tree, no page, no position.

```text
werkstatt = Place("Werkstatt", PType: 6, Rating: 2, UseFav: on)
garten = Category("Garten")
licht.room = werkstatt
```

- **Minted as Loxone Config creates them.** A new room gets `WF="16384"`, the room icon, `PType`
  (default 2, *Common Room*) and a permission group with its title; a new category the category icon and
  its group. A room's sort node goes after the folder's last one. The group and the node are pinned in
  the lock (`companions`, lockfile v10), so a recompile mints nothing new.
- **Settings:** `PType` (room type 1–6), `Rating` (stars, no attribute at 0) and `UseFav` (favourite,
  `on`/`off`, no attribute when off). They go where a save writes them, also on an adopted object that
  gains one. The icon, colour, category group and area stay with Loxone Config.
- **Named by slug or by title.** `.room` and `.category` (and a block's `room:` / `category:`) take the
  slug of a declared or extern room, checked to be a room, or a quoted title as before.
- **One title per room.** A new room or category whose title another one of its kind already has is an
  error that names the existing one's UUID: an extern or `adopt` takes that one over.
- **Renamed with its group.** A new title renames the permission group too, as Loxone Config does.
- **Removed with its group and node**, under `--allow-removals` as every removal. An object that still
  names the room or category keeps the compile from removing it: the error says how many.
- **Existing ones:** `adopt --uuid <room> --as <slug>` takes one over with its UUID, its group and its
  version (a room keeps the `V=` it was saved at). Bulk adopt leaves them to the base. An extern
  (`extern kueche = Place(uuid: …)`) names one without owning it.
- **The app catalogue** gets an entry for a new room or category in the shape Loxone Config 17.3
  writes, loses the entry of a removed one, and follows a rename and the settings. An entry it cannot
  write exactly (a custom icon, a colour, a category group) stays as it was until Loxone Config saves.
- **The drift fingerprint is unchanged.** It already holds the objects and their titles; the room
  settings are left out of the parameter comparison. Watching them for drift would need a fingerprint
  version of its own.

## Consequences

- An agent can set up a room in one push: the room, its category and the blocks in it.
- A page whose room is a new room (`page "X" (room: "Werkstatt")`) needs the room in the base: pages
  are made before the room is minted. Push the room first.
- Users and rights still give access per permission group in Loxone Config. A new room's group has no
  members.

## Evidence

Oracle session 95 ([oracle-sessions.md](../oracle-sessions.md)), the *Rooms and categories* section of
[loxone-format.md](../loxone-format.md), and `tests/rooms_categories.rs`.
