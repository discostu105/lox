# Design: `lox tui` — Interactive Terminal UI

> **Status: PROPOSED** — Design and UX proposal. Nothing implemented yet. Tracked as a bd epic.

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
| P3 | **Always discoverable** | The footer always shows what the keys do *here*. `?` shows everything. `a` opens a menu of every action on the selected item. |
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
3                  → Events screen (live feed; history since the TUI started, or from the journal, see §7.3)
/hallway⏎          → filter chip "hallway"
j/k                → select the event "Hallway · Light · 0 → 1 · 02:14:07"
⏎                  → detail pane: the control's other states that changed within ±2 s
                     (motion sensor 0 → 1 at 02:14:06 — found it)
e                  → (from a control anywhere) jump here with the control pre-filtered
```

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
| 3 | **Events** | "What just happened?" | Live · Journal |
| 4 | **Energy** | "Where does the power go?" | Now · Today · 7d · 30d |
| 5 | **System** | "Is the Miniserver healthy?" | Overview · Devices · Bus & LAN · Log · Config · Update |
| 6 | **Sites** | "How are all my Miniservers?" | — |

Hierarchy inside each screen: **Screen → Pane → Item → Action**.

- A screen has 1–3 **panes**. One pane has **focus** (accent border, bold title).
- A pane holds a list or a view with a **selected item**.
- **Actions** apply to the selected item (or to marked items, see §4.4).
- **Overlays** sit on top of all this: palette, help, action menu, confirm, value input, context switcher.
  Overlays form a stack, and `Esc` always pops one level.

Screens and panes that don't apply are hidden or show a friendly empty state. There is no Energy
screen content without meters ("No energy meters found — add a Meter block in Loxone Config"), and Sites
with a single context shows "Add another Miniserver with `lox ctx add`".
The number of a hidden screen stays reserved, so muscle memory never breaks.

---

## 4. Keyboard model

### 4.1 Rules that keep it unconfusing

1. **Navigation keys never change the house.** Arrows, `hjkl`, `g`/`G`, `Tab`, `PgUp`/`PgDn` only move.
2. **Action keys mean the same intent on every control type** (§4.3). The footer shows the concrete effect.
3. **Lowercase acts, uppercase goes wider.** `y` copies the command, `Y` the UUID. `C` switches context.
   There are no hidden chords and no `g`-prefix sequences.
4. **`Esc` always goes one level back**: close overlay → clear filter → unmark → focus parent pane.
   `Esc` never quits.
5. **`q` quits** from anywhere without an overlay (`Ctrl-C` always quits).
6. **One keymap table** in code (`tui/keymap.rs`) drives dispatch, the footer, the `?` overlay and the
   generated docs, and a unit test fails if two bindings collide in the same context.
7. Text input (`/`, `:`, `=`) captures all printable keys. `Esc` cancels, `⏎` confirms.

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
| `p` | Pause / resume live updates (the display freezes, events keep buffering) |
| `Ctrl-r` | Refresh the structure cache and reconnect |
| `q`, `Ctrl-c` | Quit |
| mouse | click selects/focuses, scroll scrolls, click on a tab switches screen (disable with `--no-mouse`) |

### 4.3 The action vocabulary

The core of P2. Eleven keys cover everything:

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
| `*` | **Pin** to Home (with sparkline) | star |
| `e` | Show this item's **events** | events |
| `y` / `Y` | Copy the `lox` **command** / UUID | yank |

Per type, the footer shows what each key does:

| Control type | `␣` | `+`/`-` | `<` / `>` | `=` | `s` | `m` |
|--------------|-----|---------|-----------|-----|-----|-----|
| Switch, Pushbutton | toggle / pulse | — | off / on | — | — | — |
| LightControllerV2 | toggle (last mood ↔ off) | brightness ±10 % of the master dimmer | off / 100 % | brightness | — | mood picker |
| Dimmer, EIBDimmer | toggle | ±10 % | 0 / 100 % | level | — | — |
| ColorPickerV2 | toggle | brightness ±10 % | off / 100 % | `#hex` or `hsv()` | — | color presets |
| Jalousie (blind) | stop if moving, else full up/down (opposite of last direction) | position ±10 % (closed %) | fully up / fully down | position | stop | shade (auto) |
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

Keys that don't apply are **shown dimmed in the footer**, not hidden, so the layout stays stable.
Pressing one gives a short hint toast ("Blind South has no modes").

### 4.4 Marking (bulk)

`v` marks or unmarks the selection, `V` marks all visible items (so `/` then `V` means "all that
match"). While anything is marked, the footer says `4 marked` and action keys apply to all
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

Mockups are **120×36** (standard layout). Color versions of every screen, with a live theme switcher, are in
[`design-tui-mockups.html`](design-tui-mockups.html). Open it in a browser.

### 5.1 Header & footer (every screen)

```
 lox  ¹Home ²Rooms ³Events ⁴Energy ⁵System ⁶Sites        home · MS Gen2 15.2 · ● live 12ms · Evening · 18.4° ☼ · 14:32
 …
 ␣ toggle  +- dim  <> off/max  = set  m mood  ⏎ inspect  a actions            : palette  / filter  ? help  q quit
```

- Left: the product mark and tabs. The active tab is in the accent color with an underline bar. Superscript
  numbers are the shortcuts, as in btop.
- Right: context · Miniserver model and firmware · **connection** (`● live 12 ms`, `◐ reconnecting 3s`,
  `○ offline — showing cached values`, `⏻ out of service (updating)`) · operating mode · outside temp · clock.
- Footer left: context-aware action keys for the focused item. Footer right: global keys.
- Toasts appear at the bottom right above the footer: `✓ Blind South → pos 30` / `✗ 403 forbidden — token user lacks rights`.
- `READ-ONLY` and `PAUSED` show as inverted badges in the header.

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
- **Pinned**: any control pinned with `*`, each with its live value and a 30-min sparkline built from the stream.
- **Quick**: scenes (`~/.lox/…/scenes`) and favorite moods. Focus the pane and press `␣` to run.
- **Live**: the last N events (the full view is on screen 3).

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
- **Right pane** is the inspector for the selected control. It has a type-specific visualization (blind
  window, thermostat dial, light color swatch, mood list), a 24 h history sparkline (if statistics are
  enabled), raw states, and the equivalent CLI command. At < 140 columns the inspector becomes an overlay on `⏎`.
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
- **Noise control** matters most at scale. High-frequency analog states (meters, power, lux) are
  **collapsed** into one row per control with `×N` and the latest value. `x` mutes a control. Mutes are
  saved per context. The header shows events per minute as a sparkline.
- The **"around this event"** correlation (±2 s, same room first) answers "why did that happen?" and costs
  nothing to compute.
- **Journal** (sub-view `]`): optionally, `lox tui` writes events to
  `~/.lox/contexts/<n>/events/YYYY-MM-DD.jsonl` (off by default, `--journal`), so you can see what
  happened at night after starting the TUI in the morning. It uses the same format as `lox stream -o json`.

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
- **Devices**: the `lox health` table (Tree/Air devices with battery, signal, last seen, status), sorted problems first.
- **Bus & LAN**: CAN/LAN counters with **per-interval deltas**. Non-zero error deltas flash red once, then stay amber.
- **Log**: tail of `/dev/fsget/log/def.log`, level-colored, `/` search, `n`/`N`.
- **Config**: the gitops repository (`lox config init/pull`). A commit list with a side-by-side diff of users,
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

**Temperature ramp** (as btop's CPU gradient): `#5cb8e6` (≤ 17 °) → `#69c350` (21 °) → `#f0a33a` (25 °) → `#ef5f5f` (≥ 28 °).
**Gauges** use a 3-stop gradient along their length in truecolor, and a solid color in 256-color mode.

Themes: `night` (default), `day` (light terminals), `mono` (only bold/dim/reverse; used automatically with
`NO_COLOR` or `--no-color`), and `neon` (a SilkCircuit-style magenta/cyan, for fun).
Truecolor is detected from `COLORTERM`, with a 256-color fallback.

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

### 6.3 Components

- **Pane**: rounded border `╭╮╰╯`. Title in the top border is left-aligned (bold when focused). Right-aligned meta
  (counts, filter) is in the top border too, as in btop.
- **Selection**: full-width `surface` background + `▸` in the accent color + bold name. Marked items get a green `▌`
  at the left edge.
- **Gauge**: `▕` + eighth-blocks `▏▎▍▌▋▊▉█` + `▏`. Smooth to 1/8 of a cell. The empty track uses `text.faint`.
- **Sparkline**: braille (2×4 dots per cell), from a 30 min or 24 h ring buffer.
- **Charts**: ratatui `Chart` with `Marker::Braille`, and a dim `┤` axis.
- **Cards**: small rounded panes. The title is the room name, the body 3–4 lines.
- **Toasts**: one line, bottom right, 3 s. Colors `ok`/`crit`/`info`. Up to 3 stacked.
- **Motion**: only for real motion (blinds, gates, energy flow, pending spinner) and the "new event" flash
  (row background fades from accent to normal in 800 ms). A calm house means a calm screen.

### 6.4 Responsive layout

| Width | Layout |
|-------|--------|
| ≥ 160 cols | **Wide**: Home adds a 4th column (weather); Rooms shows 3 panes plus a wider inspector with charts |
| 120–159 | **Standard**: the mockups above |
| 90–119 | **Compact**: Home stacks Attention/Energy under the cards; Rooms has 2 panes and an inspector overlay |
| 80–89 × ≥ 24 | **Narrow**: one pane at a time; `h`/`l` move between them (breadcrumb in the title) |
| < 80 × 24 | "Terminal too small (need 80×24)" centered, with the current size |

Height: sections collapse from the bottom (Home: Live → Quick → Pinned), and there is always a single header and footer row.

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
- **Initial burst**: the Miniserver sends every state on subscribe (thousands of events). These are applied
  to the store silently and don't appear in the event feed.

### 7.3 Connection lifecycle

`connecting → authenticating → live ⇄ reconnecting (exp. backoff 1→30 s) → out-of-service → live`.
While not live, values show in `text.dim` with a stale marker, and actions are queued for 10 s or refused
with a toast. The structure version (`/jdev/sps/LoxAPPversion3`) is checked on reconnect, and the structure
reloads when it changed (for example after a config upload).

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
  effects.rs              runs Effects: send command, poll, switch context, write journal
  store.rs                live state store: state-uuid → value, control view models, event ring buffer
  model/                  typed view models per control type (LightVm, BlindVm, ClimateVm, …)
  keymap.rs               declarative keymap: (context, key) → Command; feeds footer and help
  theme.rs                palettes, glyph sets, capability detection
  ui/                     render functions per screen + overlay
    home.rs rooms.rs events.rs energy.rs system.rs sites.rs
    palette.rs help.rs confirm.rs picker.rs input.rs
  widgets/                gauge.rs sparkline.rs card.rs flow.rs tabs.rs toast.rs blindviz.rs
  pollers.rs              diagnostics, health, energy stats, sites (spawn_blocking + LoxClient)
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

### 8.4 Dependencies

| Crate | Why | Size impact |
|-------|-----|-------------|
| `ratatui` | rendering and widgets | ~ 300 KB |
| `crossterm` (feature `event-stream`) | terminal backend and async input | small |
| `nucleo-matcher` | fuzzy matching | small |
| `insta` (dev) | snapshot tests of rendered frames | — |

Clipboard uses **OSC 52** (no dependency, works over SSH). Everything sits behind a cargo feature `tui`
(on by default) so a minimal build can drop it.

### 8.5 CLI surface

```
lox tui [--screen home|rooms|events|energy|system|sites] [-r <room>]
        [--read-only] [--journal] [--theme night|day|mono|neon] [--icons plain|nerd] [--no-mouse]
```
It follows API_DESIGN_GUIDELINES: `-r` is the room, global `--no-color` gives the `mono` theme, and `--ctx` selects the context.

Preferences go in `~/.lox/tui.yaml` (theme, icons, energy role overrides, poll intervals). Per-context
UI state goes in `~/.lox/contexts/<n>/tui-state.yaml` (pins, mutes, palette history, last screen).

---

## 9. Safety

| Risk level | Examples | Behavior |
|------------|----------|----------|
| **None** | lights, dimmers, blinds, music, thermostat target, moods, scenes | instant |
| **Confirm** | door unlock/open, gate open, alarm arm/disarm, intercom open, wallbox start, operating mode change, bulk action on > 10 controls | `y`/`N` dialog, default No |
| **Typed** | reboot, firmware update, config restore/upload | type the context name |

- `--read-only`: all action keys are disabled (footer shows them dim, with `READ-ONLY` in the header). Ideal for wall displays and screen sharing.
- Alarm PINs and secured commands use masked input and are never saved in history or the journal.
- The TUI never shows passwords or tokens. The Sites view shows the token expiry, not the token.

---

## 10. Testing

- **Action layer**: a table-driven test per control type (§4.3 table ⇄ `plan()`).
- **Keymap**: no two bindings conflict within a context. Every binding has a help text.
- **Journeys**: J1–J12 as `update()` tests with a fixture structure (≈ 40 controls, 8 rooms) and a scripted event stream.
- **Rendering**: `insta` snapshots of every screen at 80×24, 120×36 and 180×50 via `TestBackend`, in the `night` and `mono` themes.
- **Scale**: a generated fixture with 2,000 controls / 80 rooms, plus a 1,000 ev/s synthetic stream. A test asserts
  that the frame time stays under budget.
- **Manual**: a `lox tui --demo` hidden flag runs against a built-in fake Miniserver (fixture + simulated
  events). It is also used for README screenshots/GIFs (recorded with `vhs`).

---

## 11. Milestones

| # | Milestone | Content | Depends on |
|---|-----------|---------|------------|
| M0 | **Action layer** | `src/actions.rs`; CLI handlers migrated; mapping tests | — |
| M1 | **Skeleton** | `lox tui`, terminal lifecycle, Msg/update/effects loop, store fed by the stream, theme, keymap, header/footer, help overlay, `--demo` fake Miniserver | M0 |
| M2 | **Rooms** | three panes, grouping, filter, action vocabulary, value input, pickers, confirm, marks, inspector, optimistic updates | M1 |
| M3 | **Home** | room cards, attention engine, pinned + sparklines, quick scenes, live ticker | M2 |
| M4 | **Events** | feed, follow/pause, collapse, mute, detail with correlation, journal | M1 |
| M5 | **System** | overview pollers, devices, bus & LAN, log | M1 |
| M6 | **Energy** | role detection, flow widget, meters, history chart | M1 |
| M7 | **Palette** | fuzzy go-to, clap-parsed commands, completion, history, yank (OSC 52) | M2 |
| M8 | **Sites** | multi-context polling, switcher, live context switch | M1 |
| M9 | **System ops** | Config (gitops log + diff), Update/Reboot with typed confirm | M5 |
| M10 | **Polish** | themes day/mono/neon, nerd icons, mouse, responsive breakpoints, scale tests, docs (COMMANDS.md, README, GitHub Pages), GIF | all |
| M11 | **Logic (lxir)** | spike: LoxApp3 ⇄ `.Loxone` UUID mapping; semantic config diff; logic inspector with live wire values (§12.1) | M9 |

M2 alone already makes the TUI useful for control. M3 + M4 add the "wow".

---

## 12. Related projects: lxir and lox-cli

### 12.1 `discostu105/lxir`: **yes, but after the MVP and optional**

lxir compiles `.Loxone` XML into a text IR (and back), with a semantic document model (`doc`: objects,
ports, wires, pages), a verified connector table for 100+ block types, and a semantic `diff` that filters
out locale noise. It has the same author and licensing as lox (GPL-3.0 + commercial), so there is no license friction.

What it adds to the TUI:

| Where | What lxir enables | Value |
|-------|-------------------|-------|
| **System › Config** (J11) | Replace the current `loxone_xml::diff_configs` summary (users/devices/control counts) with lxir's **semantic diff**: "block `Hallway Light` gained input from `Motion 2`", "parameter `Off-delay` 120 → 300 s". | High. Turns "something changed" into "this is what changed". |
| **Logic inspector** (new, J3+) | Load the downloaded `.Loxone` through `lxir::doc` and show the **wiring around a control**: which inputs feed it and which blocks it feeds. Combined with the event correlation (§5.4), this answers "why did the light turn on?" *causally*: `Motion Hallway ──▶ Lighting controller Q1 ──▶ Hallway Light`, with the live values on each wire. | Very high. Nothing else does this, including the Loxone app. |
| Rooms inspector | Show a control's config-side parameters (off-delays, thresholds) next to its live states | Medium |

Costs and risks:

- lxir is v0 (early, ~65 commits). Its API will change. **Mitigation**: depend on it behind the cargo
  feature `logic` (off in the first TUI release). Wrap it in a thin `src/logic.rs` adapter so that API churn touches one file.
- It needs the config file: FTP download (`lox config download` / gitops repo) plus LoxCC decompression. Both exist
  already. The inspector works on the gitops checkout when there is one, and otherwise offers "download config (FTP)" once and caches it.
- **Unverified assumption**: are the control UUIDs in `LoxApp3.json` the same as the object UUIDs in the `.Loxone`
  XML (or derivable from them)? The live-values-on-wires idea depends on it. **Verify with a spike before committing to it.**
- Binary size: lxir brings its connector table and parser (probably small, but measure).

Recommendation: keep lxir out of M0–M8. Add **M11 "Logic"**: a spike to verify the UUID mapping, then the
semantic diff in System › Config, then the logic inspector. If the spike fails, the semantic diff still
works on its own.

### 12.2 `eisber/lox-cli`: **no, don't take code from it; at most borrow ideas**

lox-cli is a fork of lox (see its NOTICE) by another author. It focuses on *authoring* config with AI agents,
and has an offline SPS simulator (215 block types).

- **License is the blocker.** It is AGPL-3.0 + *their* commercial license. Copying its code into lox would put AGPL
  code into lox and **break lox's own commercial dual license**, because you can't relicense someone
  else's AGPL code commercially. Keep it clean-room: read their docs for ideas, don't copy code.
- **Little overlap with the TUI anyway.** It has no TUI, no WebSocket or live state, and it targets config
  editing, while the TUI is about runtime operation. Its simulator simulates *logic blocks*, which is not what
  `--demo` needs (a fake *Miniserver API*: structure + state stream + command endpoint). A small
  fixture-driven fake is simpler and belongs in lox.
- It is a diverged copy of the same foundations (client, config, contexts). Depending on it would mean two
  versions of the same concepts.
- The only idea worth noting: their `.github/skills/` agent docs. That concerns lox's AI-agent story, not the TUI.

---

## 13. Open questions

1. **Journal default**: should `--journal` be on by default? It is useful for J3, but it writes to disk continuously.
2. **Nerd Font default**: off (safe) or auto-detect? Proposal: off, and suggest it in the help overlay.
3. **Space on a blind**: "stop if moving, else toggle full up/down" copies the Loxone single-button behavior.
   Is that intuitive enough, or should `␣` be stop-only?
4. **Energy roles**: is auto-detection from control types reliable on real installs, or do we need `tui.yaml` mapping from day 1?
5. **Theme name**: "Loxone Night" uses the Loxone name. Is a neutral name better (trademark)?
6. **lxir timing**: is M11 late enough, or should the UUID-mapping spike run in parallel with M1 so the
   logic inspector can shape the Rooms inspector layout early?
