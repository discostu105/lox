# Design: `lox tui` — Interactive Terminal UI

> **Status: IMPLEMENTED (M0–M10)** — `lox tui` ships all six screens, the palette, the wiring overlay (lxir),
> System › Config diffs, themes, mouse and `--demo`. Tested against a real Gen 2 Miniserver (read-only) and the
> built-in demo house. §13 records the decisions on the open questions, §14 what real data changed.
> Not done yet: README GIF (`vhs`) and the 1,000 ev/s frame-time benchmark.

## 1. Vision

`lox tui` is a full-screen, keyboard-first, live view of a Loxone installation — **btop for your
house**. It shows the house as it changes right now, lets you act on it, and explains why things happen.

It is not a copy of the Loxone app. The app is good for tapping a light on a phone. The TUI is good at:

1. **Seeing everything at once**: rooms, values, energy, health, and events on one screen, updating live.
2. **Explaining**: "what just changed, where, and from what to what?"
3. **Operating quickly from the keyboard**: find any of 1,000 controls in three keystrokes and act on it.
4. **Covering what the app doesn't**: Miniserver diagnostics, Tree/Air device health, CAN/LAN errors,
   system log, config history, and several Miniservers side by side.
5. **Linking back to the CLI**: every action in the TUI has an equivalent `lox` command. You can copy it
   (`y`) or type it (`:`), so the TUI also teaches the CLI.

Design principles, in priority order:

| # | Principle | Meaning |
|---|-----------|---------|
| P1 | **Live by default** | Values come from the WebSocket stream, not polling. Nothing needs a manual refresh. |
| P2 | **One vocabulary** | A small set of keys means the same *intent* everywhere, on every control type. |
| P3 | **Always discoverable** | The focused pane's bottom border always shows what the keys do *here*. `?` shows everything. `a` opens a menu of every action on the selected item. |
| P4 | **Safe** | Harmless actions are instant. Doors, alarms and gates ask for confirmation. Reboot and restore ask you to type a word. `--read-only` exists. |
| P5 | **Scales** | 5 rooms or 80 rooms, 50 controls or 2,000, 1 event/s or 1,000: filters, grouping, virtualized lists, noise control. |
| P6 | **Beautiful, calm** | A dark, restrained palette with one accent. Color carries meaning, not decoration. Motion only where something is really moving. |

---

## 2. Personas & user journeys

### Personas

| Persona | Who | Primary screens |
|---------|-----|-----------------|
| **Resident** | Lives in the house, sits at a laptop, doesn't want to reach for the phone | Home, Rooms |
| **Tinkerer** | Owns the install, writes scenes and automations, debugs "why did that happen?" | Events, Rooms (inspector), Palette |
| **Integrator** | Installs and maintains Loxone for customers, often several sites, often over SSH | System, Sites, Events |
| **Energy nerd** | Has PV, a battery and a wallbox; wants to see power flow | Energy, Home |

### Journeys

Each journey lists the exact keystrokes. They double as acceptance tests (see §10).

**J1 — "Turn off the office lights before leaving"** (Resident)
```
lox tui            → Home
2                  → Rooms screen, focus on room list
/off⏎              → filter rooms: "Office" selected (fuzzy)
l                  → focus moves to Office's controls
(on "Ceiling")  ␣  → toggles off. Row shows ⋯ pending, then ○ off once the stream confirms.
```
Alternative for the whole room: on the room row, `a` → action menu → **All lights off**.
Power user: `:off --all-in-room Office⏎`.

**J2 — "Blinds in the living room to 30%"** (Resident)
```
: living south⏎    → palette jumps to the control "Blind South [Living room]" (Rooms screen, selected)
=30⏎               → set exact position. The gauge animates while the blind moves.
```
Alternatives: `+`/`-` step by 10%; `<` fully up, `>` fully down; `s` stop.

**J3 — "Why did the hallway light turn on at night?"** (Tinkerer)
```
3                  → Events screen (live feed, in memory since the TUI started)
/hallway⏎          → filter chip "hallway"
j/k                → select the event "Hallway · Light · 0 → 1 · 02:14:07"
⏎                  → detail pane: other states that changed within ±2 s
                     (motion sensor 0 → 1 at 02:14:06 — a suspect)
w                  → wiring overlay: Motion Hallway ━━▶ LightController2 ━━▶ Hallway Light,
                     live values on every wire — confirmed causally (§5.9)
e                  → (from a control anywhere) jump here with the control pre-filtered
```
Events are not persisted. For "what happened last night" when the TUI wasn't running, the
answer is the control's history in the inspector (Miniserver statistics) or `lox otel` into a real TSDB.

**J4 — "Is the Miniserver OK? Things feel slow."** (Integrator)
```
5                  → System › Overview: CPU/heap sparklines, tasks, context switches, SD test, uptime
]                  → System › Devices: Tree/Air devices sorted by problem (offline, battery, signal)
]                  → System › Bus & LAN: CAN/LAN counters with deltas; errors highlighted
]                  → System › Log: Miniserver log tail, level-colored, searchable with /
```

**J5 — "Which sensor needs a new battery?"** (Resident, Integrator)
Home shows it without any keystrokes. The **Attention** panel lists
`⚠ Window contact Bath · battery 8%`. `⏎` on that line jumps to System › Devices with it selected.

**J6 — "How much PV right now, and where is it going?"** (Energy nerd)
```
4                  → Energy screen: flow diagram PV → House / Battery / Grid / Wallbox,
                     meter table, and today's chart (from /binstatisticdata)
[ / ]              → chart range: today · 7 days · 30 days
```

**J7 — "Check three customer sites, then fix one"** (Integrator)
```
6                  → Sites: one row per context: status, firmware, CPU, problems, last event
j ⏎                → switch the whole TUI to that context (reconnects the stream)
C                  → from anywhere: quick context switcher popup
```

**J8 — "I want this in a script"** (Tinkerer)
```
(any control selected)  y   → copies `lox blind "Blind South" pos 30 -r "Living room"` (the last action,
                              or `lox get …` if none) to the clipboard via OSC 52, so it works over SSH
Y                           → copies the UUID
```

**J9 — "Arm the alarm when leaving"** (Resident)
```
: alarm⏎           → selects the alarm control
m                  → mode picker: Arm · Arm (home) · Arm without motion
⏎                  → confirmation dialog: "Arm alarm 'House'? [y/N]"
```
Disarm needs the PIN in a masked input when the control requires it.

**J10 — "Run my 'movie' scene"** (Resident)
```
:movie⏎            → the palette shows scenes (from ~/.lox/…/scenes) → runs it; a toast shows each step
```
Scenes, moods and autopilot rules also appear in the Home **Quick** panel.

**J11 — "What changed in the config since last week?"** (Integrator)
```
5 ]]]]             → System › Config: git log from `lox config pull` (gitops)
⏎                  → diff viewer (users, devices, controls diff), scroll with j/k, n/N next/prev hunk
```

**J12 — "Wall display in the hallway"** (Resident)
`lox tui --read-only --screen home` on a Raspberry Pi with a small screen. The compact layout
kicks in, no actions are possible, and the header says `READ-ONLY`.

---

## 3. Information architecture

Six top-level **screens**, always in the same order, always on the number keys:

```
 ¹Home   ²Rooms   ³Events   ⁴Energy   ⁵System   ⁶Sites
```

| # | Screen | Question it answers | Sub-views (`[` / `]`) |
|---|--------|---------------------|-------------------------|
| 1 | **Home** | "How is my house right now? Does anything need me?" | — |
| 2 | **Rooms** | "Show me / let me change anything." | Group by: Room · Category · Type (`b` cycles) |
| 3 | **Events** | "What just happened?" | — |
| 4 | **Energy** | "Where does the power go?" | Now · Today · 7d · 30d |
| 5 | **System** | "Is the Miniserver healthy?" | Overview · Devices · Bus & LAN · Log · Config · Update |
| 6 | **Sites** | "How are all my Miniservers?" | — |

Hierarchy inside each screen: **Screen → Pane → Item → Action**.

- A screen has 1–3 **panes**. One pane has **focus** (accent border, bold title).
- A pane holds a list or a view with a **selected item**.
- **Actions** apply to the selected item (or to marked items, see §4.4).
- **Overlays** sit on top of all this: palette, help, action menu, confirm, value input, context switcher, wiring.
  Overlays form a stack, and `Esc` always pops one level.

Screens and panes that don't apply are hidden or show a friendly empty state. There is no Energy
screen content without meters ("No energy meters found — add a Meter block in Loxone Config"), and Sites
with a single context shows "Add another Miniserver with `lox ctx add`".
The number of a hidden screen stays reserved, so muscle memory never breaks.

---

## 4. Keyboard model

### 4.1 Rules that keep it unconfusing

1. **Navigation keys never change the house.** Arrows, `hjkl`, `g`/`G`, `Tab`, `PgUp`/`PgDn` only move.
2. **Action keys mean the same intent on every control type** (§4.3). The key hints show the concrete effect.
3. **Lowercase acts, uppercase goes wider.** `y` copies the command, `Y` the UUID. `C` switches context.
   There are no hidden chords and no `g`-prefix sequences.
4. **`Esc` always goes one level back**: close overlay → clear filter → unmark → focus parent pane.
   `Esc` never quits.
5. **`q` quits** from anywhere without an overlay (`Ctrl-C` always quits).
6. **One keymap table** in code (`tui/keymap.rs`) drives dispatch, the border key hints, the `?` overlay and the
   generated docs, and a unit test fails if two bindings collide in the same context.
7. Text input (`/`, `:`, `=`) captures all printable keys. `Esc` cancels, `⏎` confirms.
8. **One press, one action.** Only key *press* events act (crossterm also reports release, on Windows and with
   the kitty keyboard protocol). Auto-repeat is allowed for movement and `+`/`-` (coalesced into one command per
   frame), and ignored for `␣`, `<`/`>`, `=` and anything that asks for confirmation, so holding Space
   can't toggle a blind back and forth.
9. **Paste is text, never a submit.** Bracketed paste into the palette or a value input inserts the text; a pasted
   newline does not run the command. Only a real `⏎` press confirms.
10. **Back restores place.** `Esc`/`h` out of a room and back in restores that room's selection and scroll position
    (kept per room for the session).

### 4.2 Global keys

| Key | Action |
|-----|--------|
| `1`–`6` | Go to screen |
| `Tab` / `Shift-Tab` | Next / previous pane |
| `h` `l` / `←` `→` | Focus pane left / right (lazygit-style) |
| `j` `k` / `↓` `↑` | Move selection |
| `g` / `G`, `Home` / `End` | First / last |
| `Ctrl-d` / `Ctrl-u`, `PgDn` / `PgUp` | Half page down / up |
| `[` / `]` | Previous / next sub-view (sub-tabs, chart ranges) |
| `/` | Filter the focused list (incremental, fuzzy). `Esc` clears. |
| `:` or `Ctrl-k` | **Palette**: go to anything, or run a `lox` command |
| `?` | Help overlay for the current context |
| `C` | Context (site) switcher |
| `!` | Message log: failures and connection events of this session (§5.1) |
| `p` | Pause / resume live updates (the display freezes, events keep buffering) |
| `Ctrl-r` | Refresh the structure cache and reconnect |
| `q`, `Ctrl-c` | Quit |
| mouse | click selects/focuses, scroll scrolls, click on a tab switches screen (disable with `--no-mouse`) |

### 4.3 The action vocabulary

The core of P2. Twelve keys cover everything:

| Key | Intent | Mnemonic |
|-----|--------|----------|
| `␣` Space | **Primary action**: the thing you most often do | "press the button" |
| `+` / `-` | **Step** the main value up / down | "+ increases the number you see" |
| `<` / `>` | **Minimum / maximum** of the main value | arrows point to the ends |
| `=` | **Set exact** value (inline prompt) | "equals" |
| `s` | **Stop** motion / playback | stop |
| `m` | **Mode / mood** picker | mode |
| `a` | **Action menu**: every action for this item, including rare ones | actions (right-click) |
| `⏎` | **Inspect**: open the inspector / drill in | open |
| `w` | **Wiring**: the logic around this control or event, with live values (§5.9) | "why?" |
| `*` | **Pin** to Home (with sparkline) | star |
| `e` | Show this item's **events** | events |
| `y` / `Y` | Copy the `lox` **command** / UUID | yank |

Per type, the key hints show what each key does:

| Control type | `␣` | `+`/`-` | `<` / `>` | `=` | `s` | `m` |
|--------------|-----|---------|-----------|-----|-----|-----|
| Switch, Pushbutton | toggle / pulse | — | off / on | — | — | — |
| LightControllerV2 | toggle (last mood ↔ off) | brightness ±10 % of the master dimmer | off / 100 % | brightness | — | mood picker |
| Dimmer, EIBDimmer | toggle | ±10 % | 0 / 100 % | level | — | — |
| ColorPickerV2 | toggle | brightness ±10 % | off / 100 % | `#hex` or `hsv()` | — | color presets |
| Jalousie (blind) | stop if moving, else full travel opposite to the last direction (hint shows `▲ up` / `▼ down` / `stop`) | position ±10 % (closed %) | fully up / fully down | position | stop | shade (auto) |
| Gate | stop if moving, else open/close | — | open / close | — | stop | — |
| IRoomControllerV2 | — | target ±0.5 °C | eco / comfort temp | target °C | — | auto · eco · comfort · manual |
| Alarm ⚠ | — | — | — | — | — | arm · arm home · arm w/o motion · disarm |
| Door lock ⚠ | lock ↔ unlock | — | — | — | — | open |
| Intercom | answer / hang up | — | — | — | — | open door |
| Wallbox (charger) | start ↔ pause | limit ±1 kWh | — | limit | stop | — |
| Music zone | play ↔ pause | volume ±5 | mute / max | volume | stop | source / favorite |
| Analog / virtual input | pulse | ±step (from the control's min/max/step) | min / max | value | — | — |
| Operating mode (global) | — | — | — | — | — | mode picker |
| Room (row in the rooms list) | enter the room | — | all lights off / on | — | stop all blinds | room mood |
| Scene | run | — | — | — | — | — |
| Sensors / meters (read-only) | — | — | — | — | — | — |

The border hints list only the keys that apply (the `?` overlay and `a` menu show the rest). Pressing a key
that doesn't apply gives a short hint toast ("Blind South has no modes").

**Decision — `␣` on a blind** (was open question 3): `␣` means "press the button", and the Loxone single-button
behavior *is* the button. So: while the blind moves, `␣` stops it (the same rule as gate and music: `␣` during
motion always stops). At rest, it travels fully in the direction opposite to the last one. The ambiguity is removed
by the hint, which is computed from the live state and always says what will happen: `␣ stop`, `␣ ▲ up` or
`␣ ▼ down`. Exact positioning stays on `+ - < > =`; `s` is an explicit stop that never starts motion.

### 4.4 Marking (bulk)

`v` marks or unmarks the selection, `V` marks all visible items (so `/` then `V` means "all that
match"). While anything is marked, the pane's bottom border says `4 marked` and action keys apply to all
marked items. Keys that apply to only some of them act on those and report `3 of 4 applied`.
`Esc` clears the marks.

### 4.5 Screen-specific keys

The few screen-specific keys don't collide with the vocabulary above:

| Screen | Key | Action |
|--------|-----|--------|
| Rooms | `b` | Cycle group-by: Room · Category · Type |
| Rooms | `f` | Toggle "favorites only" (Loxone `isFavorite` + pins) |
| Events | `f` | Toggle filter chips bar (type / room / category) |
| Events | `x` | Mute the selected control's events (noise control); `X` shows muted |
| Events | `F` | Follow mode on/off (auto-scroll to newest) |
| System › Log | `n` / `N` | Next / previous match |
| System › Config | `n` / `N` | Next / previous diff hunk; `P` = `lox config pull` now |
| System › Update | `R` | Reboot (typed confirmation); `U` install update (typed confirmation) |

### 4.6 Palette (`:`)

One entry point for "go to" and "do":

```
╭─ : blind south▌ ───────────────────────────────────────────────╮
│ CONTROLS                                                        │
│ ▸ ▾ Blind South          Living room   Jalousie     33% closed  │
│   ▾ Blind South          Kids room     Jalousie      0%         │
│ COMMANDS                                                        │
│   lox blind "Blind South" …   up · down · stop · shade · pos N  │
╰───────────────────────── ⏎ go  ⇥ complete  Esc close ──────────╯
```

- Plain text → fuzzy results grouped as **Controls · Rooms · Scenes · Screens · Sites · Actions**.
- Text that starts with a `lox` control verb (`on`, `off`, `blind`, `light`, `thermostat`, `run`, …)
  is parsed with the **same clap definitions** as the CLI and runs in-process. `⇥` completes names.
  The `lox` prefix is optional. Only control and `run` verbs are allowed. Anything that
  prints (for example `ls`) shows "use the Rooms screen" instead.
- `↑`/`↓` in an empty palette browses history (kept in `tui-state.yaml`).

---

## 5. Screens

Mockups are **120×36** (standard layout). The **color mockups are the visual reference**:
[`design-tui-mockups.html`](design-tui-mockups.html) renders every screen as an exact cell grid with a live theme
switcher. The ASCII sketches below show structure and content; for glyph-level look (gradients, braille graphs,
notched titles) follow the HTML.

### 5.1 Header & pane chrome (every screen)

```
 lox  ¹home ²rooms ³events ⁴energy ⁵system ⁶sites ───────── 14:32:07 ───────── home ● live 12ms  evening  18.4° ☼  ? help
╭┐²rooms┌┐by room┌──────╮╭┐Living room┌┐/ filter┌──────────────────── ┐14┌─╮
│ …                      ││ …                                              │
╰────────────── ┘1/18└──╯╰┘␣ stop└┘+- step└┘= set└┘m shade└┘w wiring└─ ┘5/14└─╯
```

There is **no footer row**. Following btop, everything lives in the borders:

- **Header row**: the `lox` mark, then the screen tabs with superscript shortcuts (active one in accent, bold,
  underlined). A rule runs across with the **clock centered in it**. Right: context · **connection**
  (`● live 12 ms`, `◐ reconnecting 3s`, `○ offline — cached values`, `⏻ out of service`) · operating mode ·
  outside temp · `? help`. `READ-ONLY` and `PAUSED` show as inverted badges here.
- **Title notches** `┐title┌` in the top border: the pane name (the screen's first pane carries the superscript
  number, `┐²rooms┌`), then sub-tabs or modes as further notches. The **hotkey letter inside a notch is
  highlighted** in the accent color (`┐by r`**`o`**`om┌`, `┐`**`/`**` filter┌`), so each notch documents its own key.
  Right-aligned notch: meta (counts, poll interval `┐poll 2 s┌`, `┐self-use 86%┌`).
- **Hint notches** `┘key label└` in the **bottom border of the focused pane only**: the context-aware action
  keys, computed per selected item from the keymap (`␣ stop` on a moving blind, `␣ ▲ up` at rest). Unfocused panes
  show only a right-aligned position notch (`┘1/18└`). Global keys live in `?`. This replaces the footer, saves a
  row, and puts the hints next to what they act on.
- Toasts appear inside the focused pane's bottom-right corner: `✓ Blind South → pos 30` / `✗ 403 forbidden — token user lacks rights`.
- **A toast is never the only record.** Every failure (action refused, command error, reconnect, poll failure) also
  goes into an in-memory **message log**. While it holds unread errors, the header shows a `! 2` badge; `!` opens
  the log as an overlay (time, what, error, the `lox` command that failed). It is part of the same ring-buffer
  memory budget and is not persisted.

### 5.2 ¹ Home — the dashboard

```
╭─ Rooms ───────────────────────────────────────────────────────────╮╭─ Attention ─────────────────────────────╮
│ ╭ Living room ──────╮ ╭ Kitchen ──────────╮ ╭ Office ───────────╮ ││ ⚠ Bath window open · rain in 20 min     │
│ │ 22.1° → 22.0°  ▲  │ │ 21.4° → 22.0°  ▲  │ │ 22.9° → 21.5°  ▼  │ ││ ⚠ Window contact Bath · battery 8%      │
│ │ ● 3 lights  62 %  │ │ ● 1 light  100 %  │ │ ● 2 lights  40 %  │ ││ ● Front door unlocked since 13:02       │
│ │ ▾ blinds ▕██▎  ▏  │ │ ▾ blinds ▕     ▏  │ │ ▾ blinds ▕████▌▏  │ ││ ✓ Miniserver healthy · 0 bus errors     │
│ ╰───────────────────╯ ╰───────────────────╯ ╰───────────────────╯ │╰─────────────────────────────────────────╯
│ ╭ Bedroom ──────────╮ ╭ Bath ─────────────╮ ╭ Garage ───────────╮ │╭─ Energy ────────────────────────────────╮
│ │ 19.8° → 19.0°  ·  │ │ 23.5° → 24.0°  ▲  │ │ 14.2°          ·  │ ││  PV  ▕██████████▌   ▏ 6.2 kW ▁▂▄▆█▇▆    │
│ │ ○ lights off      │ │ ○ lights off      │ │ ● 1 light         │ ││ Home ▕████▏           2.8 kW ▃▃▄▃▅▃▃    │
│ │ ▾ blinds ▕█████▏  │ │ ◫ window open  ⚠  │ │ ⌂ gate closed     │ ││ Batt ▕███████▌  ▏ 74 %  +1.3 kW ▲       │
│ ╰───────────────────╯ ╰───────────────────╯ ╰───────────────────╯ ││ Grid ▕██▏         ⇢ -2.1 kW export      │
│                                          … 14 more  ⏎ open Rooms  │╰─────────────────────────────────────────╯
╰───────────────────────────────────────────────────────────────────╯╭─ Pinned ────────────────────────────────╮
╭─ Live ────────────────────────────────────────────────────────────╮│ ● Terrace light    on 80 %               │
│ 14:32:07  Kitchen      Light Ceiling      0 → 100                  ││ ° Outside temp     18.4°  ▂▃▃▄▅▆▆▅       │
│ 14:32:05  Hallway      Motion             ○ → ●                    ││ ▾ Blind South      33 % ▕███▎      ▏     │
│ 14:31:58  Living room  Blind South        0 % → 42 %   ▼ moving    ││ ⚡ Wallbox          charging 11 kW        │
│ 14:31:40  —            Operating mode     Day → Evening            │╰─────────────────────────────────────────╯
╰───────────────────────────────────────────────────────────────────╯╭─ Quick ─────────────────────────────────╮
                                                                     │ ▶ movie   ▶ leave   ▶ night   ▶ morning  │
                                                                     ╰─────────────────────────────────────────╯
```

- **Room cards** are a responsive grid (2–5 columns). Card order: pinned rooms, then by "activity"
  (lights on, recent events), then alphabetical. Card border color = room temperature trend
  (blue cool · green in band · amber warm). Unfocused cards are dim; the selected card gets the accent border.
  `⏎` on a card goes to Rooms with that room selected.
- **Attention** is the key idea on Home: a ranked list of anything that needs a human.
  Sources: device health (battery < 20 %, offline, weak signal), windows open + rain or wind warnings
  from global states, doors unlocked > 30 min, alarm events, Miniserver problems (CPU > 80 %, SD errors, bus errors),
  firmware update available, token expiring. An empty list shows `✓ All good`, and says so with some pride.
- Round 2 visuals: each card has a one-row **dot sparkline** of the room temperature, colored along the
  temperature gradient; blinds show as an `info`-gradient meter. The Energy panel gets a gradient meter plus
  a dot sparkline per flow.
- **Pinned**: any control pinned with `*`, each with its live value and a type-fitting mini visual
  (meter for levels, dot sparkline for temperatures, dot meter for capacities), built from the stream.
- **Quick**: scenes (`~/.lox/…/scenes`) and favorite moods as key-caps. Focus the pane and press `␣` to run.
- **Live**: the last N events. Rows **fade with age** (the `fade` gradient, bright → faint), and the newest
  flashes briefly. The title notch carries the event rate as a dot sparkline (the full view is on screen 3).

### 5.3 ² Rooms — the control surface

```
╭─ Rooms ─────────────── 18 ╮╭─ Living room ──────────────────────────── 14 controls ╮╭─ Inspector ──────────────────────────╮
│ ★ Favorites            6  ││ LIGHTS                                                 ││ Blind South                          │
│   All controls       312  ││ ● Ceiling        ▕██████▌   ▏  65 %   mood Evening     ││ Jalousie · Living room · Shading     │
│ ─────────────────────────  ││ ● Reading lamp   ▕██████████▏ 100 %                    ││                                      │
│ ▸ Living room  22.1° ●3 ▾ ││ ○ Floor LEDs     ▕          ▏   off                    ││   ▲ ┌──────────┐                      │
│   Kitchen      21.4° ●1   ││ SHADING                                                ││     │██████████│  position   33 %     │
│   Office       22.9° ●2 ▾ ││ ▾ Blind South    ▕███▎      ▏  33 %   ▼ moving   ⋯      ││     │███▎      │  slats      45 %     │
│   Bedroom      19.8°      ││ ▾ Blind West     ▕          ▏   0 %                    ││     │          │  auto shade  on      │
│   Bath         23.5°    ⚠ ││ CLIMATE                                                ││   ▼ └──────────┘                      │
│   Kids room    21.0° ●1   ││ ° Room climate    22.1° → 22.0°   comfort   ▲ heating  ││                                      │
│   Hallway      20.4°      ││ ° Humidity        48 %                                 ││ 24 h  ▁▁▁▁▁▅█████▃▁▁▁▁▁▁▂▅██▇▅▃      │
│   Garage       14.2°      ││ SENSORS                                                ││                                      │
│   Garden        —         ││ ◫ Window left     closed                               ││ STATES                               │
│   …                       ││ ◫ Window right    tilted                           ⚠   ││ position      0.33                   │
│                           ││ ◉ Presence        present  since 13:48                 ││ shadePosition 0.45                   │
│                           ││ AUDIO                                                  ││ up            0     down   1         │
│                           ││ ♪ Living zone     ▶ Radio FM4   vol 35                 ││ autoActive    1     locked 0         │
│                           ││                                                        ││                                      │
│                           ││                                                        ││ y  lox blind "Blind South" pos 30    │
╰───────────────────────────╯╰────────────────────────────────────────────────────────╯╰──────────────────────────────────────╯
```

- **Left pane** lists the rooms with their temperature, `●N` lights on, `▾` blinds not fully open, and `⚠` attention.
  Two pseudo-entries at the top: **★ Favorites** and **All controls**. `b` switches the grouping to categories or
  types (the left list then shows categories or types).
- **Middle pane** lists the controls, grouped by category with section headers (LIGHTS · SHADING · CLIMATE …).
  Each row is: glyph · name · **value visual** · value text · secondary state · pending/lock marker.
- **Right pane** is the inspector for the selected control, with notch tabs `┐states┌┐wiring┌┐history┌`.
  It has a type-specific visualization (blind window, thermostat dial, light color swatch, mood list), a
  24 h history as a small **braille graph** (Miniserver statistics if enabled, else what the TUI has seen),
  raw states, and the equivalent CLI command. The *wiring* tab is the compact form of §5.9.
  At < 140 columns the inspector becomes an overlay on `⏎`.
- Room rows color their temperature along `grad.temp`. Control rows use gradient meters
  (`lamp` for lights, `info` for blinds/humidity, `acc` for volume).
- Values move smoothly: the blind gauge animates from the stream's position updates. A command sent but not yet
  confirmed shows `⋯` for up to 3 s, then either updates or shows `✗` and reverts.
- Locked controls (`lox lock`) show `🔒`-free text `locked` in amber, and action keys are disabled with the reason.

### 5.4 ³ Events — the live feed

```
╭─ Events ─ ● following ─ 1,284 buffered ─ 38/min ▁▂▁▃▇▂▁▁▂▅ ─────────────────────── filter: room:Hallway  type:Light ╮
│ TIME      ROOM         CONTROL           STATE        CHANGE                                                     │
│ 14:32:07  Kitchen      Light Ceiling     value        0 → 100                                        ▲           │
│ 14:32:05  Hallway      Motion            active       ○ → ●                                                      │
│ 14:31:58  Living room  Blind South       position     0.00 → 0.42                                ▼ ×12 collapsed │
│ 14:31:40  —            Operating mode    mode         Day → Evening                                              │
│ 14:31:12  Office       Room climate      tempActual   22.8 → 22.9                                                │
│ 14:30:55  Garage       Gate              position     closed → open                                       ⚠      │
├─ Detail ─────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Hallway · Motion · active 0 → 1 at 14:32:05.412                                                                   │
│ Around this event (±2 s):  14:32:05.9 Hallway Light 0 → 1   ·   14:32:06.1 Hallway Light brightness 0 → 40         │
│ ⏎ open control   e only this control   x mute   y copy `lox stream -c "Motion" -r Hallway`                          │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

- A ring buffer of 10,000 events. **Follow mode** stays pinned to the newest event. Moving the selection up
  turns follow off, and `F` or `G` turns it back on. `p` pauses rendering.
- **Paused or scrolled back, the view holds still.** New events never drag the viewport or the selection; instead the
  bottom border shows a `┘↓ 37 new└` notch (with the time of the oldest unseen event), and `G` jumps to them.
  The selection is tracked by event identity, not row index, so it stays on the same event as rows arrive.
- **Noise control** matters most at scale. High-frequency analog states (meters, power, lux) are
  **collapsed** into one row per control with `×N` and the latest value. `x` mutes a control. Mutes are
  saved per context. The header shows events per minute as a sparkline.
- The **"around this event"** correlation (±2 s, same room first) suggests *what* happened together and costs
  nothing to compute. `w` on an event opens the wiring (§5.9), which shows whether it is actually *causal*.
- Round 2: a 3-row **braille event-rate graph** (events/min, `acc` gradient) spans the top of the pane, so
  bursts are visible at a glance. Rows fade with age; correlated rows get an `info` dot in the gutter.
- **No persistence.** Events live only in the in-memory ring buffer and are gone when the TUI exits. The only
  things written to disk are small UI state (mutes, pins, palette history in `tui-state.yaml`) and caches
  (structure, the downloaded `.Loxone` for §5.9). Long-term history belongs in `lox otel` → a real TSDB,
  or in the Miniserver's own statistics, which the inspector already reads.

### 5.5 ⁴ Energy

```
╭─ Energy ─ Now ─────────────────────────────────────────────────────────────────────────────────────────────────╮
│                                                                                                                 │
│        ╭────────────╮                    ╭────────────╮                    ╭────────────╮                       │
│        │  ☼  PV     │ ━━━━━━▶━━━━━━▶━━━━ │  ⌂  Home   │ ◀━━━━━━━━━━━━━━━━ │  ≋  Grid   │                       │
│        │  6.2 kW    │                    │  2.8 kW    │  ━━━━━━▶━━━━━━▶━━ │ -2.1 kW    │  exporting            │
│        ╰─────┬──────╯                    ╰─────┬──────╯                    ╰────────────╯                       │
│              ┃ ▼ 1.3 kW                        ┃ ▼ 11.0 kW                                                       │
│        ╭─────┴──────╮                    ╭─────┴──────╮                                                          │
│        │  ▮ Battery │                    │  ⏚ Wallbox │                                                          │
│        │  74 %  ▲   │                    │  charging  │                                                          │
│        ╰────────────╯                    ╰────────────╯                                                          │
├─ Today ─────────────────────────────────────────────── kWh ─┬─ Meters ───────────────────────────── 12 ────────┤
│ 8 ┤                 ⣀⣤⣶⣿⣿⣿⣷⣦⣄                                │ Heat pump       1.9 kW   ▕████▌    ▏  14.2 kWh    │
│ 4 ┤          ⣀⣤⣴⣶⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣤⣀        PV                   │ Wallbox        11.0 kW   ▕██████████▏  22.0 kWh    │
│ 0 ┼⠤⠤⠤⠤⠤⠤⠤⠤⣀⣀⣀⣀⣀⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⠤⣀⣀⣀     Grid                │ Kitchen         0.4 kW   ▕▊         ▏   3.1 kWh    │
│   00    04    08    12    16    20    24                     │ …                                                 │
╰──────────────────────────────────────────────────────────────┴───────────────────────────────────────────────────╯
```

- The flow diagram is built from the configured meters. Roles (PV, grid, battery, wallbox, home) are
  detected from control types (`EnergyManager2`, `EFM`, `Meter` with bidirectional type, `Wallbox2`) and
  can be overridden in `tui.yaml`. Flow arrows animate at a speed proportional to the power; a direction change flips them.
- Round 2: nodes get a **dot meter** (btop mem style, `⣿⣿⣿⣀⣀`) in their role gradient. The **Today** chart is a
  **mirrored braille graph** (btop's net graph): PV above the axis in the `pv` gradient, consumption below in the
  `use` gradient, a `┊` marker at *now*, and the forecast drawn in `text.faint` after it. The meter table gets
  a gradient load meter plus a 1 h dot sparkline per meter.
- If the installation has no energy manager, the screen falls back to the **Meters** table and the chart only.

### 5.6 ⁵ System

Sub-views are shown as a tab strip in the pane title: `Overview · Devices · Bus & LAN · Log · Config · Update`.

**Overview** (btop's home ground):
```
╭─ System ─ ▸Overview  Devices  Bus & LAN  Log  Config  Update ──────────────────────────────────────────────────────╮
│ CPU   ▕██████▌           ▏ 38 %  ⣀⣀⣤⣤⣀⣀⣠⣤⣶⣤⣀⣀⣀⣤⣤⣀⣀⣀⣀⣠⣤⣤⣀⣀     Miniserver Gen 2 · 504F94A0XXXX                     │
│ Heap  ▕█████████████▊    ▏ 71 %  12.4 / 17.5 MB ⣤⣤⣤⣤⣤⣤⣤⣶⣶⣶⣶⣶      firmware 15.2.10.4  ✓ up to date                  │
│ Tasks 214    Ctx/s 9.1 k    Ints/s 1.2 k    SD ✓ 0 errors            uptime 41 d 3 h · local time ✓ in sync (Δ 0.2 s)  │
├─ Network ───────────────────────────────────┬─ Extensions ───────────────────────────────────────────────────────────┤
│ IP 192.168.1.20/24  GW .1  DNS .1 · 9.9.9.9 │ ● Tree Extension        fw 15.2   ● Air Base   fw 15.2              │
│ MAC 50:4F:94:A0:XX:XX   DHCP off   NTP ✓    │ ● Extension 1           fw 15.2   ○ DI Ext     offline since 09:12 ⚠│
╰─────────────────────────────────────────────┴────────────────────────────────────────────────────────────────────────╯
```
Round 2 layout (see HTML): the Overview is a **large CPU braille graph** (11 rows, `load` gradient by height:
green → yellow → red, exactly btop's cpu box) with an inset box listing cpu, heap, tasks, ctx/s, ints/s and
comints, each as gradient meter + value + dot sparkline. Devices, Bus & LAN and Log sit below as panes; at
Standard width they are visible together, `[`/`]` still cycles the focused sub-view.

- **Devices**: the `lox health` table (Tree/Air devices with battery as a dot meter colored by level, signal bars, status), sorted problems first.
- **Bus & LAN**: CAN/LAN counters with **per-interval deltas** and a trend dot sparkline. Non-zero error deltas flash red once, then stay amber.
- **Log**: tail of `/dev/fsget/log/def.log`, level-colored, `/` search, `n`/`N`.
- **Config**: the gitops repository (`lox config init/pull`). Changed values are highlighted at the word level
  (`Off-delay 120 → `**`300`**` s`), not just as whole changed lines. A commit list with a side-by-side diff of users,
  devices and controls. `P` runs a pull now. If the repo isn't set up: a hint with `lox config init`.
- **Update**: firmware check, changelog link, `U` install and `R` reboot, both with **typed confirmation**
  (type the context name) and a progress/reconnect view through the out-of-service states.

Polling: the Overview polls diagnostics every 2 s while the view is visible, and every 30 s (heap/CPU only)
otherwise. It never runs faster than 1 s, because Gen 1 Miniservers are slow.

### 5.7 ⁶ Sites

```
╭─ Sites ──────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│   CONTEXT     HOST                 STATUS         FW          CPU   HEAP   ATTENTION           LAST EVENT         │
│ ▸ home   ◆   192.168.1.20         ● live 12ms    15.2.10.4   38 %  71 %   2 ⚠                 14:32 Kitchen light │
│   office      office.example.dyn   ● up 48ms      15.2.10.4   12 %  55 %   ✓                   —                   │
│   parents     10.8.0.12 (vpn)      ○ unreachable  14.5.12.7    —     —    token expires 2 d ⚠  —                   │
╰───────────────────────────────────────────────────────────── ◆ active   ⏎ switch   a actions (ctx use, token) ──╯
```
Only the active context has a stream. The others are polled cheaply (`/jdev/cfg/version`,
`/jdev/sys/heap`, `/jdev/sys/lastcpu`) every 15 s, and only while this screen is open.

### 5.8 Overlays

| Overlay | Trigger | Notes |
|---------|---------|-------|
| Palette | `:` / `Ctrl-k` | §4.6 |
| Help | `?` | Two columns: this context's keys, then global. Generated from the keymap. `/` searches the help. |
| Action menu | `a` | Every action for the item, each with its key and CLI equivalent. `j`/`k` + `⏎`, or press the letter. |
| Value input | `=` | Inline in the row: `position ▏30▕ %  (0–100) ⏎ set  Esc cancel`. Shows the valid range. |
| Picker | `m` | List of moods/modes; the current one is marked `●`. |
| Confirm | risky actions | `y`/`N` with the default on **No**. Level 2 (reboot, update, restore) needs the context name typed. |
| Context switcher | `C` | Small fuzzy list of contexts. |
| Wiring | `w` | §5.9 |

### 5.9 Wiring overlay (`w`) — "why is this on?"

```
╭┐Hallway Light┌┐wiring┌──────────────────────────────────────────────────────┐why is this on?┌─╮
│ ● Hallway Light  on 40%   since 14:32:05.9 · triggered by Motion Hallway 0 → 1                  │
│                                       ╭┐LightController2┌───╮                                   │
│ ◉ Motion Hallway    ━━━━━━━━━1━━━━━━━▶│Mv              AQ1 │━━━━━━40%━━━━━━━▶ ● Hallway Light 40%│
│ ◫ Door contact      ━━━━━━━open━━━━━━▶│Tg                  │                                   │
│ ☾ Night mode Memory ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄▶│DisP            AQ2 │━━━━━━0━━━━━━━━━▶ ○ Hallway LEDs off │
│ ☼ Brightness Garden ━━━━━22.0k lx━━━━▶│Br               Qp │┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄▶ · Stairs light     │
│                                       ╰────────────────────╯                                   │
│ ━━▶ live value from the stream     ┄┄▶ block without visualization: topology only               │
╰┘⏎ follow wire└┘h l upstream · downstream└┘e events└┘Esc close└──────────────┘lxir · r50.Loxone└─╯
```

- Opened with `w` on a control (Rooms, Home, palette result) or on an event (Events). It shows the **block that
  drives the control**, every input wire with its source, and every output wire with its sink, one hop each way.
  `h`/`l` move the center one block upstream/downstream; `⏎` on a wire follows it.
- **Live values on wires**: a wire's source connector UUID *is* the state UUID the WebSocket streams (verified,
  §12.1). Live wires are solid in the source's color and carry the current value; the wire that fired last pulses
  briefly. Wires from blocks without a visualization have no stream value and are drawn dashed `┄┄▶` in
  `text.faint` with their topology only. On a real config this is 13 % of all wires, but it covers the wires
  that matter most here: sensors in, controls out.
- Source: the `.Loxone` file from the gitops checkout (`lox config pull`) if present, otherwise the TUI offers
  "download config via FTP" once and caches it per context (structure-version keyed). Parsed with lxir.
- Header line: the control's current value, since when, and the input whose change came last before it
  ("triggered by"), from the event buffer.

---

## 6. Visual design

### 6.1 Palette — "Loxone Night" (default)

Loxone green is the only accent. Everything else is neutral or carries meaning.

| Token | Truecolor | Use |
|-------|-----------|-----|
| `bg` | `#0f1216` | Background (or terminal default when `transparent: true`) |
| `surface` | `#161b22` | Selected row background, overlays |
| `border` | `#2a313c` | Unfocused borders |
| `border.focus` | `#69c350` | Focused pane border + title |
| `text` | `#d7dde5` | Primary text |
| `text.dim` | `#7d8896` | Labels, secondary values, dimmed keys |
| `text.faint` | `#4b5563` | Grid lines, empty gauge tracks |
| `accent` | `#69c350` | Loxone green: active tab, selection bar, "on" |
| `on` | `#f5c451` | Light "on" glyphs (warm) |
| `ok` | `#69c350` | Healthy |
| `warn` | `#f0a33a` | Attention |
| `crit` | `#ef5f5f` | Errors, alarms |
| `info` | `#5cb8e6` | Links, pending, cool temperature |
| `energy.pv` | `#f5c451` | PV |
| `energy.grid` | `#a78bfa` | Grid |
| `energy.battery` | `#34d399` | Battery |
| `energy.load` | `#fb923c` | Consumption |

**Gradients** (round 2). Each theme defines a small set of named 2–4 stop gradients. Graphs are colored **by
height** (row position), meters **by position along the bar**: a meter at 30 % is all green, one at 95 % runs
green → yellow → red. This is btop's trick for making values readable from color alone.

| Gradient | Stops (night) | Used for |
|----------|---------------|----------|
| `grad.load` | `#69c350` → `#e8c33c` → `#ef5f5f` | CPU, heap, meter load, battery-low inverse |
| `grad.temp` | `#4aa3df` (≤ 14 °) → `#69c350` → `#f0a33a` → `#ef5f5f` (≥ 28 °) | temperatures, room sparklines |
| `grad.pv` | `#6b5212` → `#e0a93a` → `#ffe08a` | PV production graph/meters |
| `grad.use` | `#6b3413` → `#c86a2c` → `#fbab6c` | consumption (mirrored graph, meters) |
| `grad.grid` / `grad.batt` | `#3d3470` → `#a78bfa` / `#0f4d3a` → `#34d399` | grid, battery, capacities |
| `grad.info` | `#1d4660` → `#5cb8e6` | blinds, humidity |
| `grad.lamp` | `#6b5212` → `#f5c451` → `#fff1c2` | light levels |
| `grad.acc` | `#27491f` → `#69c350` → `#b8f59b` | event rates, generic counters |
| `grad.fade` | `text` → `text.faint` | event age (newest bright) |

In truecolor, gradients are precomputed into 101-entry lookup tables per theme (cheap per cell). In 256-color
mode they are quantized to the xterm cube; with 16 colors they collapse to the gradient's semantic color
(`ok`/`warn`/`crit` bands for `load`). `mono` uses bold/normal/dim bands.

Themes: `night` (default), `day` (light terminals), `mono` (only bold/dim/reverse/underline), and `neon`
(a SilkCircuit-style magenta/cyan, for fun).

**Color resolution order** (user intent before detection):
1. `--theme` / `--no-color` flags, then `theme:` in `tui.yaml`: explicit choices win (a theme flag can deliberately
   opt back into color even with `NO_COLOR` set).
2. A non-empty `NO_COLOR` → `mono`. Checked *before* any capability detection.
3. Detection: truecolor from the terminal backend / terminfo, with `COLORTERM` treated as a hint, not a requirement;
   otherwise 256 colors, otherwise 16.

Turning color off never turns off bold, underline, layout or glyphs. **Focus must survive without color**: in `mono`
the focused pane uses heavy borders (`┏━┓┃┗┛`) and a bold title, other panes light borders; the selected row keeps
its `▌` bar and bold name. Status is always paired with a glyph or word (`● on`, `⚠ battery`), never hue alone.

### 6.2 Glyphs

The default glyphs are **single-width Unicode, not emoji**, because emoji widths break alignment in many
terminals. A Nerd Font set is available with `--icons nerd` or `icons: nerd` in `tui.yaml`.

| Concept | Default | Nerd |
|---------|---------|------|
| light on / off | `●` / `○` | `󰌵` / `󰌶` |
| blind | `▾` | `󰷛` |
| temperature | `°` | `󰔏` |
| window / door | `◫` | `󰖶` |
| presence | `◉` | `󰋑` |
| music | `♪` | `󰎈` |
| energy | `≋` `☼` `▮` `⏚` | `󰚥` `󰖙` `󰁹` `󰄌` |
| pending | `⋯` (animated `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) | same |
| attention | `⚠` | `` |

### 6.3 Components — six widgets

Every screen is built from **six small widgets** (each ~50–150 lines, each with its own snapshot test). No
per-screen drawing tricks; a new screen composes these.

| Widget | Look | Notes |
|--------|------|-------|
| **NotchBox** | rounded `╭╮╰╯`, title/tab notches `┐title┌` on top, hint notches `┘k label└` at the bottom (focused only), right-aligned meta/count notches | The only pane primitive. Takes `title`, `tabs`, `meta`, `hints`, `count`, `focus`. Hotkey letters in notches are accent + bold. |
| **Meter** | full blocks with an eighth-block tip `▏▎▍▌▋▊▉`, track in `surface`-tinted `trk` | Each filled cell takes the gradient color of *its position*. |
| **DotMeter** | `⣿⣿⣿⣿⣀⣀⣀` | btop mem style, for capacities (battery, heap, charge). Filled dots gradient, empty dots `text.faint`. |
| **DotSpark** | one-row braille, 2 samples per cell, 4 levels | For per-row trends (rooms, pinned, meters, counters). Color from the gradient by the cell's value. |
| **BrailleGraph** | area graph, 2×4 dots per cell, N rows | Colored by row height. Options: `down` (grows downward, for mirrored graphs), `floor` (faint baseline dots), overlays (a *now* marker, a forecast drawn faint). Replaces ratatui's `Chart`. |
| **FlowLine** | `━━━▶━━━━━▶` with moving arrowheads | Speed ∝ value, direction flips with sign. Energy flow and the pulsing live wire in §5.9. |

Plus the plain bits: **selection** (full-width `surface` bg, accent `▌`/`▸`, bold name), **key-caps** (` ␣ ` on a
`key` background) for Quick chips and dialogs, and **toasts** (one line in the focused pane's bottom-right, 3 s, up to 3).

**Numbers beside every graph.** A braille graph or sparkline is never the only carrier of a value: the current value
(with unit) is always printed next to it, so screen readers, `mono` and screenshots stay meaningful.

**Unavailable is not zero.** A value that hasn't arrived yet, or whose source is offline, shows `—` in `text.faint`,
never `0`. Polled values older than two poll intervals show their age (`38 % · 12 s ago`) in `text.dim`.

**Motion** only where something really moves: blinds, gates, energy flow, the pending spinner, the pulse on the
wire that just fired, and the new-event flash (row background fades from accent in 800 ms). A calm house means a calm screen.
Nothing blinks. `motion: off` in `tui.yaml` (or `--no-motion`) replaces every animation with its final state; animation
never delays input or hides the current value.

### 6.4 Responsive layout

| Width | Layout |
|-------|--------|
| ≥ 160 cols | **Wide**: Home adds a 4th column (weather); Rooms shows 3 panes plus a wider inspector with charts |
| 120–159 | **Standard**: the mockups above |
| 90–119 | **Compact**: Home stacks Attention/Energy under the cards; Rooms has 2 panes and an inspector overlay |
| 80–89 × ≥ 24 | **Narrow**: one pane at a time; `h`/`l` move between them (breadcrumb in the title) |
| < 80 × 24 | "Terminal too small (need 80×24)" centered, with the current size; `q`, `Ctrl-c` and `Ctrl-z` still work, and state is kept for when it grows again |

Height: sections collapse from the bottom (Home: Live → Quick → Pinned), and there is always a single header row.
Graphs shrink in rows before they disappear. When a bottom border is too short for all hint notches, the lowest-priority
hints drop first (the keymap orders them) and a `┘? more└` notch is kept.

### 6.5 The btop visual language, and what we deliberately leave out

Adopted from btop, because it adds information density *and* looks great:

1. Braille graphs with height gradients (CPU, PV, event rate, history).
2. Mirrored up/down graphs for two opposing quantities (PV vs. consumption).
3. Gradient meters and dotted `⣿` meters.
4. Titles, sub-tabs and hotkeys as notches in the border, superscript screen numbers.
5. Key hints in the bottom border of the focused pane instead of a footer.
6. Clock and poll interval in borders (`┐poll 2 s┌`).
7. Per-row mini dot sparklines.

Left out on purpose: per-theme ASCII art/logos, animated backgrounds, box-in-box-in-box nesting beyond one inset
level, and user-editable layouts. They cost maintenance and add nothing a resident needs.

---

## 7. Scale & performance

### 7.1 Targets

| Metric | Target |
|--------|--------|
| Start to first frame (cached structure) | < 300 ms |
| Start to live values | < 1.5 s (WS auth + initial state burst) |
| Idle CPU (quiet house) | < 1 % |
| Render | event-driven, max 30 fps, coalesced |
| Controls | 2,000 without lag (virtualized lists, render only visible rows) |
| Event rate | 1,000 ev/s sustained without UI stalls (batched store updates per frame) |
| Memory | < 50 MB with a full 10 k event buffer |

### 7.2 Large-installation UX

- **Filter first**: every list supports `/`. Fuzzy matching uses `nucleo-matcher` (as in Helix), and matched
  characters are highlighted.
- **Group-by** (`b`) and **Favorites** (`f`) keep 80 rooms manageable. Room list rows are one line each.
- **Collapsed noise** in Events (§5.4), and mutes are saved.
- **Summaries before detail**: room rows and cards show aggregates (lights on, blinds, temp, ⚠), so you
  don't need to open 40 rooms to find the one with a light on. The room list can be **sorted by "activity"** (`o` cycles sort: name · activity · temperature).
- **Selection follows identity, not position.** Every list tracks its selection by UUID (events by sequence id).
  Live sorts (activity, temperature, power) are **frozen while the pane has focus** and re-apply when focus leaves
  or on `o`, so rows never jump under the cursor. After a filter, a deletion or a resize, the scroll is clamped
  and the selected item stays visible.
- **Initial burst**: the Miniserver sends every state on subscribe (thousands of events). These are applied
  to the store silently and don't appear in the event feed.

### 7.3 Connection lifecycle

`connecting → authenticating → live ⇄ reconnecting (exp. backoff 1→30 s) → out-of-service → live`.
While not live, values show in `text.dim` with a stale marker, and actions are queued for 10 s or refused
with a toast. The structure version (`/jdev/sps/LoxAPPversion3`) is checked on reconnect, and the structure
reloads when it changed (for example after a config upload).

**Stale results are discarded.** Every Effect carries a `(context generation, request id)` tag, and so does its
result Msg. `update()` drops results whose context generation is no longer current (after `C` / a Sites switch or
a reconnect) or whose request was superseded (a newer poll, a newer value input for the same control). A closed
view cancels its pollers; a late result can't resurrect it.

**Pending writes are tracked by identity.** A command sent for a control marks it `⋯` with the request id. A
conflicting action on the same control (another `␣` while the first is unconfirmed) is coalesced into the pending
one or waits for it, instead of starting a second write. A timeout is shown as "unconfirmed" (`?`), not as a
rollback, because the command may have reached the Miniserver; the next stream value reconciles the state.

**Untrusted text.** Control and room names, log lines, device names and anything else read from the Miniserver or the
config are display data: control characters and escape sequences are stripped before rendering, so a crafted name can't
move the cursor, set the window title or trigger OSC 52.

---

## 8. Architecture

### 8.1 Shared action layer (prerequisite refactor)

Today the per-type command strings (`FullUp`, `manualPosition/30`, mood IDs, thermostat mode numbers, …) live
inside the CLI handlers. Before the TUI, they move to **`src/actions.rs`**:

```rust
pub enum Action { On, Off, Toggle, Pulse, Step(f64), Set(f64), Min, Max, Stop,
                  Mood(MoodRef), Mode(String), Color(Color), Blind(BlindAction), … }

pub fn plan(control: &Control, state: &ControlState, action: &Action) -> Result<Vec<Command>>;
pub fn to_cli(control: &Control, action: &Action) -> String;   // for `y` and toasts
pub fn risk(control: &Control, action: &Action) -> Risk;       // None | Confirm | Typed
```

The CLI handlers call the same `plan()`, so CLI and TUI can never disagree. The mapping table in §4.3 is
exactly this function, and it gets table-driven unit tests.

### 8.2 Modules

```
src/actions.rs            shared: Action → Loxone commands, CLI rendering, risk level
src/tui/
  mod.rs                  entry (`lox tui`), terminal setup/restore, panic hook
  app.rs                  App state: screen, focus, overlay stack, marks, toasts
  msg.rs                  Msg enum (Key, Mouse, Resize, Tick, State(Vec<StateEvent>), Poll(..), CmdResult(..))
  update.rs               pure `update(app, msg) -> Vec<Effect>` (Elm-style, testable)
  effects.rs              runs Effects: send command, poll, switch context, load config for wiring
  store.rs                live state store: state-uuid → value, control view models, event ring buffer
  model/                  typed view models per control type (LightVm, BlindVm, ClimateVm, …)
  keymap.rs               declarative keymap: (context, key) → Command; feeds hint notches and help
  theme.rs                palettes, gradient LUTs, glyph sets, capability detection
  ui/                     render functions per screen + overlay
    home.rs rooms.rs events.rs energy.rs system.rs sites.rs
    palette.rs help.rs confirm.rs picker.rs input.rs wiring.rs
  widgets/                notchbox.rs meter.rs dotmeter.rs dotspark.rs braille.rs flow.rs   (§6.3)
  pollers.rs              diagnostics, health, energy stats, sites (spawn_blocking + LoxClient)
src/logic.rs              thin adapter over lxir: load .Loxone, index by UUID, neighborhood(uuid) → blocks + wires
```

### 8.3 Runtime

```
 crossterm EventStream ─┐
 ws stream_events() ────┼──▶ mpsc<Msg> ──▶ update() ──▶ Effects ──▶ tokio tasks / spawn_blocking(LoxClient)
 pollers (interval) ────┤                     │                              │
 tick (animation, 60ms)─┘                     ▼                              └──▶ Msg::CmdResult / Msg::Poll
                                         dirty? ─▶ draw (≤ 30 fps)
```

- Reuses `stream::ws_authenticate`/`stream_events` (a small refactor to accept a channel sender and a
  shutdown signal) and `build_state_uuid_map`.
- All HTTP goes through the existing blocking `LoxClient` in `spawn_blocking`.
- `update()` is pure, so journeys J1–J12 become unit tests: feed a key sequence, assert the Effects.
- **One owner for input**: only the crossterm `EventStream` reads the terminal (no `poll`/`read` anywhere else).
- **Terminal lifecycle**: a guard object owns raw mode, alternate screen, cursor, mouse capture, bracketed paste
  and the keyboard-enhancement flags, and restores all of them on normal exit, error, panic (panic hook) and
  SIGTERM/SIGHUP. `Ctrl-z` restores the terminal, suspends, and on `fg` re-acquires it and redraws in full.
  Frames are wrapped in synchronized output when the terminal supports it.

### 8.4 Dependencies

| Crate | Why | Size impact |
|-------|-----|-------------|
| `ratatui` | rendering and widgets | ~ 300 KB |
| `crossterm` (feature `event-stream`) | terminal backend and async input | small |
| `nucleo-matcher` | fuzzy matching | small |
| `lxir` | `.Loxone` document model, wires, semantic diff (§12.1) | small (serde, serde_json, sha2, thiserror; all but sha2 already in the tree) |
| `unicode-width` | display width of names (wide/combining characters) | small |
| `libc` | SIGTERM/SIGHUP → restore the terminal | already in the tree |

`insta` was planned for snapshot tests; the render tests assert invariants instead (§10), which survive visual polish
without churning snapshot files.

Clipboard uses **OSC 52** (no dependency, works over SSH). Everything sits behind a cargo feature `tui`
(on by default) so a minimal build can drop it.

### 8.5 CLI surface

```
lox tui [--screen home|rooms|events|energy|system|sites] [-r <room>]
        [--read-only] [--theme night|day|mono|neon] [--icons plain|nerd] [--no-mouse] [--no-motion] [--demo]
```
`--demo` is a **visible** flag (not hidden as first planned): it is the quickest way to try the TUI without a
Miniserver, and the demo house is fully synthetic.
It follows API_DESIGN_GUIDELINES: `-r` is the room, global `--no-color` gives the `mono` theme, and `--ctx` selects the context.

Preferences go in `~/.lox/tui.yaml` (theme, icons, `motion`, `mouse`, `transparent`, energy role overrides under `roles:`). Per-context
UI state goes in `~/.lox/contexts/<n>/tui-state.yaml` (pins, mutes, palette history, last screen).

---

## 9. Safety

| Risk level | Examples | Behavior |
|------------|----------|----------|
| **None** | lights, dimmers, blinds, music, thermostat target, moods, scenes | instant |
| **Confirm** | door unlock/open, gate open, alarm arm/disarm, intercom open, wallbox start, operating mode change, bulk action on > 10 controls | `y`/`N` dialog, default No |
| **Typed** | reboot, firmware update, config restore/upload | type the context name |

- `--read-only`: all action keys are disabled (hint notches show only navigation, with `READ-ONLY` in the header). Ideal for wall displays and screen sharing.
- Alarm PINs and secured commands use masked input and are never saved in palette history.
- The TUI never shows passwords or tokens. The Sites view shows the token expiry, not the token.

---

## 10. Testing

- **Action layer**: a table-driven test per control type (§4.3 table ⇄ `plan()`).
- **Keymap**: no two bindings conflict within a context. Every binding has a help text.
- **Journeys**: J1–J12 as `update()` tests with a fixture structure (≈ 40 controls, 8 rooms) and a scripted event stream.
- **Rendering**: every screen and the main overlays at 60×20, 80×24, 120×36 and 180×50 in `night`, `day` (256 colors)
  and `mono`, with all poll results loaded: no panics, the header intact, the frame exactly the terminal size.
  Widgets have unit tests for edge values (0 %, 100 %, eighth-block tips, empty series). Implemented as invariant
  checks rather than `insta` snapshots (§8.4).
- **Wiring**: a small checked-in `.Loxone` fixture; test that `logic::neighborhood()` resolves control → block → wires and that live/dashed classification matches the structure fixture.
- **State edge cases** as `update()` tests: selection survives re-sort and new events; stale results after a context
  switch are dropped; a closed view ignores late results; a held `␣` sends one command; a pasted newline doesn't submit;
  a failed action appears in the message log.
- **Scale**: a generated fixture with 2,000 controls / 80 rooms, plus a 1,000 ev/s synthetic stream. A test asserts
  that the frame time stays under budget.
- **Real PTY smoke test** (snapshots don't exercise terminal modes): run `lox tui --demo` in a pseudo-terminal,
  send keys, paste and a resize, quit, and assert the terminal is restored (echo on, cursor visible, main screen).
- **Hostile text**: a fixture whose names contain escape sequences and wide/combining characters renders clean and aligned.
- **Live probe** (ignored by default): `LOX_TUI_LIVE=1 cargo test live_probe -- --ignored` opens a read-only
  stream to the configured Miniserver and checks that the initial value dump arrives.
- **Manual**: `lox tui --demo` runs against a built-in fake Miniserver (fixture + simulated
  events). It is also used for README screenshots/GIFs (recorded with `vhs`).

---

## 11. Milestones

| # | Milestone | Content | Depends on |
|---|-----------|---------|------------|
| M0 | **Action layer** | `src/actions.rs`; CLI handlers migrated; mapping tests | — |
| M1 | **Skeleton** | `lox tui`, terminal lifecycle, Msg/update/effects loop, store fed by the stream, theme + gradients, the six widgets, keymap + hint notches, header, help overlay, `--demo` fake Miniserver | M0 |
| M2 | **Rooms** | three panes, grouping, filter, action vocabulary, value input, pickers, confirm, marks, inspector, optimistic updates | M1 |
| M3 | **Home** | room cards, attention engine, pinned + sparklines, quick scenes, live ticker | M2 |
| M4 | **Events** | rate graph, feed, follow/pause, collapse, mute, detail with correlation (in-memory only) | M1 |
| M5 | **System** | overview pollers, devices, bus & LAN, log | M1 |
| M6 | **Energy** | role detection, flow widget, meters, history chart | M1 |
| M7 | **Palette** | fuzzy go-to, clap-parsed commands, completion, history, yank (OSC 52) | M2 |
| M8 | **Sites** | multi-context polling, switcher, live context switch | M1 |
| M5b | **Wiring (lxir)** | `src/logic.rs` over lxir, config fetch + cache, wiring overlay `w` (§5.9), inspector *wiring* tab | M2, M4 |
| M9 | **System ops** | Config (gitops log + **lxir semantic diff**), Update/Reboot with typed confirm | M5, M5b |
| M10 | **Polish** | themes day/mono/neon, nerd icons, mouse, responsive breakpoints, scale tests, docs (COMMANDS.md, README, GitHub Pages), GIF | all |
| (later) | **What-if** | optional: simulate "what would happen if…" via `lox-sim` behind a feature flag (§12.2) | M5b |

M2 alone already makes the TUI useful for control. M3 + M4 add the "wow", and M5b the "nobody else can do this".
The former M11 spike is gone: the UUID mapping is verified (§12.1).

---

## 12. Related projects: lxir and lox-cli

### 12.1 `discostu105/lxir`: **yes — add it, as a regular dependency for M5b**

lxir compiles `.Loxone` XML into a text IR (and back), with a semantic document model (`doc`: objects,
ports, wires), a verified connector table for 100+ block types, and a semantic `diff` that filters
out locale noise. It has the same author and licensing as lox, and tiny dependencies
(`serde`, `serde_json`, `sha2`, `thiserror`).

**The round-1 assumption is verified.** Checked against a real installation (structure cache of 148 controls
and 25 rooms vs. its `.Loxone`, plus two other config copies):

| LoxApp3.json | `.Loxone` XML | Match |
|--------------|---------------|-------|
| control UUID | `<C U=…>` block UUID | **148 / 148** |
| room UUID | room `<C U=…>` | **25 / 25** |
| state UUID (what the WebSocket streams) | output connector `<Co K=… U=…>` of the block | **469 / 950**; the rest are virtual states that exist only in the API (`jLocked`, `moodList`, `infoText`, `targetPosition`, …) |
| — | wire = `<In Input="source-Co-UUID"/>` inside the sink connector | the source UUID **is** the streamed state UUID |

Examples: Meter `actual` → `OPf`, `total` → `OMr`; Jalousie `position` → `OutputPos`, `shadePosition` → `OutputLPos`;
Pushbutton `active` → `Q`. So a wire's live value can be looked up directly in the store by its source UUID —
no heuristic mapping, no name matching.

**Coverage caveat.** The config has 1,933 blocks but only 148 are visualized controls. Of 891 wires, **116 (13 %)**
have a streamed source. Internal logic (AND/OR gates, memory flags, math) has no live value; the wiring overlay
shows those wires dashed, topology only (§5.9). That is honest and still useful: the wires people ask about
("which sensor drives this light?") are exactly sensor → controller → output, and those are live.

What lxir gives the TUI, with the API it already has:

| Where | lxir API | Value |
|-------|----------|-------|
| **Wiring overlay** `w` (§5.9) and the inspector *wiring* tab | `LoxoneDoc::parse`, `objects()`, `ports(el)` with `inputs`, `wires()` | Very high. Answers "why is this on?" causally, with live values. Nothing else does this, including the Loxone app. |
| **System › Config** (J11) | `diff::diff(a, b)` → added/removed/renamed blocks, `param_changes`, `wires_added/removed` | High. "block `Hallway Light` gained input from `Motion 2`", "`Off-delay` 120 → 300 s" instead of counts. Replaces `loxone_xml::diff_configs`. |
| Rooms inspector | `ports()` parameters | Medium: config-side parameters (off-delays, thresholds) next to live states. |

Recommendation: **add it.** Drop the spike; schedule it as **M5b** right after Events (§11), behind a thin
`src/logic.rs` adapter so lxir API churn (it is v0) touches one file. No separate cargo feature is needed given
its size; if the binary grows noticeably, fold it under the existing `tui` feature. Measure the size in M5b.

### 12.2 `eisber/lox-cli`: reassessed without license concerns — **still no code merge; maybe `lox-sim` later**

Ignoring licensing (you'd sort that out with the maintainer), the question is purely technical. A deep look at the
current tree (~301 commits since 2026-04; last commit 2026-09-08):

- **Shape.** A workspace: the `lox` binary (~23k LOC) and a `lox-sim` crate (~31k LOC). It *added* config editing
  (`config_edit/`, ~7.4k LOC), a simulator, blocks/color/telemetry commands. It *removed* the runtime layer the
  TUI needs: `stream.rs`, `otel.rs`, `scene.rs`, control commands, inspect, system.
- **Shared code.** `client.rs`, `ftp.rs`, `ctx.rs` are byte-identical to ours; `main.rs`, `token.rs`, `gitops.rs`,
  `ws.rs` have diverged heavily. Nothing there is ahead of lox for runtime use.
- **Runtime reuse for the TUI: none.** No per-type action mapping, no state decoding, no binary event parsing.
- **Config model vs. lxir.** Heavy overlap with lxir's `doc`. lxir is the more rigorous one (byte-faithful
  round-trip, UUID identity) and is already what we'd use. lxir itself already uses lox-cli as its simulation
  transport (`ir/simtest.rs`), so the two ecosystems are linked on the *authoring* side, not the runtime side.
- **The simulator (`lox-sim`)** is the one genuinely interesting part: `.Loxone` → `SimGraph` → topological
  tick engine, `set_input`/`get_output`/`trace`, snapshot/restore, 224 registered block types (logic, timer, math
  solid; big controllers like `LightController2` only approximated; many stubs), ~500 tests. Signals are `f64`
  only, and blocks are addressed by title (the UUID is not kept), so mapping to our state UUIDs is new glue.

How it would fit the three places it could matter:

| Use | Fit | Why |
|-----|-----|-----|
| `--demo` fake Miniserver | **No** | Needs LoxApp3 JSON synthesis, API-command → block-input translation and output → state events; weeks of glue for approximate controllers. A fixture + scripted stream is simpler and exact. |
| Filling the 87 % dashed wires in §5.9 | **Tempting, but no** | Simulated values next to live ones would be presented as truth while controllers are approximated. A wiring view must never guess. |
| **What-if** ("if motion fires now, what turns on?") | **Plausible, later** | Snapshot, `set_input`, tick N, diff outputs, clearly badged *simulated* (`BlockSupport::Unimplemented` exists for honest gaps). |

**Assessment changes, but only a little.** Without the license blocker, the answer moves from "don't touch it" to:
don't merge the fork back and don't depend on the `lox-cli` binary crate (it dropped everything the TUI needs);
if a what-if feature is ever wanted, depend on **`lox-sim` as a versioned crate behind a feature flag** (ask the
maintainer to publish current releases — crates.io still has an April 0.1.0 — and upstream fixes there). Check the
binary-size and dependency cost (rayon, a different `xmltree` major) before doing so. Nothing from lox-cli is needed
for M0–M10.

---

## 13. Decisions (formerly open questions)

Resolved in round 2: event journal (dropped — in-memory only), `␣` on a blind (decided, §4.3),
theme name ("Loxone Night" stays), lxir timing (M5b, no spike needed).

1. **Nerd Font default** — **off**, opt-in only via `--icons nerd` / `tui.yaml`. There is no reliable way
   to detect a Nerd Font; the help overlay mentions the option.
2. **Energy roles** — **auto-detect, with `tui.yaml` overrides from day 1.** An EFM's node types (Grid, Production,
   Storage) are reliable; without an EFM, meter type and name heuristics apply. `roles:` in `tui.yaml` maps a
   meter name or UUID to `grid | pv | battery | load` for installs where both fail. The real test install was
   detected correctly without overrides.
3. **No footer** — **no footer, not even for first sessions.** The header always shows `? help`, and the focused
   pane's hint notches cover the keys in reach. A first-run footer would need a start counter and would still
   disappear before people know the TUI. Revisit if users report missing keys.
4. **Config freshness for wiring** — **show it, with a notch.** The cached `.Loxone` is stored with the structure
   version. When the running version differs, the wiring overlay shows *cached · config may be stale* and `d`
   re-downloads. A wiring view that is one edit behind is still far more useful than none, and the notch is honest.
5. **What-if** — **not now.** The live wiring view answers the actual question ("why is this on?"). Keep §12.2 as the
   plan if people ask for it.

---

## 14. Revisions from real data

Found by running against a real Gen 2 Miniserver (read-only) and its structure file (1,092 value states):

- **Stream framing bug (also in `lox stream`).** Each message is an 8-byte header plus a payload. Command replies
  arrive as a *text* frame, which did not clear the pending header, so the next binary frame was parsed as a header
  and the initial value dump was mostly lost (74 of 1,092 states arrived). `stream::Framer` now owns the header
  state for all three stream loops; 790 states arrive (the rest are text/daytimer states that come in their own tables).
- **Meters have no statistics** on this install; temperature sensors do, and they are **hourly**. The energy
  *today* chart therefore uses only statistics whose output is a power value and otherwise builds the day from the
  **session** (notch *this session*). Statistics timestamps are local wall time, not UTC; the parser converts with
  the current UTC offset.
- **EFM values arrive only on change.** `actual0–4` may never arrive in a session, so node power falls back to the
  node meter's `actual`, then to the EFM's `Gpwr` / `Ppwr` / `Spwr`. Self-use is `(PV − export) / PV`.
- **Gen 2 lacks some counters** (`ctx/s`, `ints/s`, `comints`); System hides counters the Miniserver doesn't report
  instead of showing `—`.
- **`sdtest` exercises the SD card**, so it is polled at most every 10 minutes.
- **Light-circuit switches** report `active` as 0/100 instead of 0/1; the view normalizes to on/off.
- **Duplicate names are the norm** (many rooms have a "Temperatur"). The palette matches name *and* room, and the
  CLI copy (`y`) always includes `-r <room>`.
- **Noise**: meters, EFM, analog and text states change constantly. They stay out of the Home live feed, and
  changes below display precision don't create events.
- **The `def.log` tail can be months old**; the Log view opens on the newest line, like `tail`.
- **gitops commit subjects** carry a `[ms]` prefix; System › Config strips it.
- **Value input (`=`)** is prefilled with the current value *selected*, so `=` `30` `⏎` sets 30 (typing replaces it,
  arrows keep it for editing).
