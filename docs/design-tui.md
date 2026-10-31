# Design: `lox tui`

`lox tui` is a full-screen, keyboard-first, live view of a Loxone installation. It shows the house as it changes,
lets you act on it, and explains why things happen. The user guide is [guides/tui.md](guides/tui.md). This page is
the design that the code in `src/tui/` follows.

## Principles

In priority order:

1. **Live by default.** Values come from the WebSocket stream. Nothing needs a manual refresh.
2. **One vocabulary.** A small set of keys means the same intent on every control type.
3. **Always discoverable.** The focused pane's bottom border shows the keys in reach. `?` shows all of them, and `a`
   opens every action on the selected item.
4. **Safe.** Harmless actions are instant. Risky ones ask first (see [Safety](#safety)).
5. **Scales.** 80 rooms, 2,000 controls and 1,000 events/s stay usable through filters, grouping and virtualized
   lists.
6. **Calm.** One accent color. Color carries meaning. Motion only where something really moves.

Every action has an equivalent `lox` command: `y` copies it and `:` runs it, so the TUI also teaches the CLI.

## Screens

Six screens, always in this order, always on the number keys:

| # | Screen | Question it answers | Sub-views (`[` / `]`) |
|---|--------|---------------------|-----------------------|
| 1 | Home | How is my house right now? Does anything need me? | — |
| 2 | Rooms | Show me or let me change anything. | Group by room, category or type (`b`) |
| 3 | Events | What just happened? | — |
| 4 | Energy | Where does the power go? | Now, today, 7 days, 30 days |
| 5 | System | Is the Miniserver healthy? | Overview, devices, bus and LAN, log, config, update |
| 6 | Sites | How are all my Miniservers? | — |

Inside a screen the hierarchy is screen → pane → item → action. A screen has one to three panes, and one has focus.
Overlays (palette, help, action menu, confirm, value input, context switcher, wiring, chart) form a stack on top.

A screen that doesn't apply shows an empty state that says what to do ("Add another Miniserver with `lox ctx add`").
Its number stays reserved.

## Keyboard rules

1. Navigation keys never change the house.
2. Action keys mean the same intent on every control type. The border hints show the concrete effect.
3. Lowercase acts, uppercase goes wider: `y` copies the command, `Y` the UUID. There are no chords.
4. `Esc` (or `⌫`) goes one step back and never quits. Inside an overlay it steps back first, then closes it. On a
   screen it clears the filter, then the facets, then the marks, then returns to the screen a jump came from.
5. `q` closes the overlay at once, however deep. On a screen it quits. `Ctrl-C` always quits.
6. One keymap table (`keymap.rs`) drives dispatch, the border hints and the `?` overlay. A test fails if two
   bindings collide in the same context.
7. Text input (`/`, `:`, `=`) captures all printable keys. `Esc` cancels and `⏎` confirms.
8. One press, one action. Only key presses act. Auto-repeat works for movement and `+`/`-` (one command per frame)
   and is ignored for `␣`, `<`/`>`, `=` and anything that asks for confirmation.
9. Pasted text is text. A pasted newline never submits.
10. Going back restores the place: each room keeps its selection and scroll position for the session.

## Action vocabulary

| Key | Intent |
|-----|--------|
| `␣` | Primary action: toggle, pulse, or for blinds and gates stop while moving, else travel |
| `+` / `-` | Step the main value up or down |
| `<` / `>` | Minimum or maximum of the main value |
| `=` | Set an exact value (prefilled with the current one, selected) |
| `s` | Stop motion or playback; never starts motion |
| `m` | Mode or mood picker |
| `a` | Action menu with every action, including rare ones |
| `⏎` | Inspect or drill in |
| `w` | Wiring: the logic around this control, with live values on the wires |
| `c` | History chart |
| `*` | Pin to Home |
| `e` | Show this item's events |
| `y` / `Y` | Copy the `lox` command or the UUID (OSC 52, so it works over SSH) |

The mapping from intent to Loxone command per control type lives in `src/actions.rs`, shared with the CLI and
`lox mcp`, so the front-ends can't disagree. A key that doesn't apply shows a short toast ("Blind South has no
modes"). `v` marks items and `V` marks all visible ones. Action keys then apply to all marked items and report
`3 of 4 applied`.

`/` filters a list by name. `f` opens facets: values grouped by dimension (room, category, type, state) with match
counts. Values of one dimension combine with OR, dimensions with AND.

The palette (`:`) does "go to" and "do". Plain text gives fuzzy results (controls, rooms, scenes, screens, sites).
Text that starts with a control verb (`on`, `blind`, `run`, …) is parsed with the CLI's own clap definitions and
runs in-process. Commands that print, like `ls`, are refused with a hint.

## Visual design

[`design-tui-mockups.html`](design-tui-mockups.html) renders every screen as a cell grid with a theme switcher.

Themes: `night` (default), `day`, `mono` and `neon`. Loxone green is the only accent. Each theme names its colors
by meaning (`ok`, `warn`, `crit`, `energy.pv`, …) and defines a few gradients. Graphs are colored by height and
meters by position along the bar, so a meter at 95 % runs from green to red.

Color is chosen in this order:

1. `--theme` and `--no-color`, then `theme:` in `tui.yaml`.
2. A non-empty `NO_COLOR` gives `mono`.
3. Detection: truecolor, else 256 colors, else 16. Gradients are quantized to fit.

Turning color off keeps bold, layout and glyphs. In `mono` the focused pane gets heavy borders. Status always pairs
a glyph or word with its color (`● on`, `⚠ battery`). Default glyphs are single-width Unicode, not emoji, because
emoji widths break alignment. Nerd Font icons are opt-in (`--icons nerd`), since no terminal reports them.

Every screen is built from six widgets in `src/tui/widgets/`:

| Widget | Look |
|--------|------|
| NotchBox | The only pane primitive: rounded border, title and tab notches on top, key hints at the bottom when focused |
| Meter | Full blocks with an eighth-block tip, each cell colored by its position |
| DotMeter | `⣿⣿⣿⣀⣀` for capacities (battery, heap) |
| DotSpark | One-row braille sparkline for per-row trends |
| BrailleGraph | Area graph, colored by row height, optionally mirrored |
| FlowLine | `━━▶━━▶` with arrowheads that move at a speed set by the value |

- A graph always has its current value printed next to it.
- A value that hasn't arrived yet shows `—`, never `0`. Polled values older than two intervals show their age.
- Motion is limited to blinds, gates, energy flow, the pending spinner and the new-event flash. `--no-motion` shows
  the final state instead. Nothing blinks.

Layout follows the width: wide (160+ columns) adds panes, standard is 120–159, compact (90–119) stacks panes, narrow
(80–89) shows one pane at a time. Below 80×24 the TUI shows the needed size and keeps its state. When a border is
too short for all hints, the lowest-priority ones drop first.

## Robustness

- **Selection follows identity.** Lists track the selection by UUID, events by sequence number. Live sorts are
  frozen while the pane has focus, so rows don't jump under the cursor.
- **Stale results are dropped.** Every effect carries the epoch it started under. `update()` drops results from an
  older epoch, which a context switch or structure refresh starts.
- **Pending writes.** A sent command marks the control `⋯`. A second action on it waits for or joins the first. A
  timeout shows as "unconfirmed", because the command may have reached the Miniserver. The next stream value
  settles it.
- **Untrusted text.** Names, log lines and anything else from the Miniserver or the config are stripped of control
  characters and escape sequences before rendering (`text.rs`).
- **Connection.** The states are connecting, authenticating, live and reconnecting (backoff 1 to 30 s). While not
  live, values are dimmed. On reconnect the structure version is checked, and the structure reloads when it changed.
- **Initial burst.** The Miniserver sends every state on subscribe. These update the store without entering the
  event feed.

## Architecture

The core is Elm-style. `update(app, msg) -> Vec<Effect>` is pure, so user journeys are unit tests.

| Module | Role |
|--------|------|
| `app.rs` | App state, `Msg`, `Effect`. `update.rs` is the only writer |
| `update.rs` | The pure update function |
| `run.rs` | Terminal runtime: owns the terminal and the event loop |
| `exec.rs` | Runs effects against the live Miniserver or the `--demo` simulator |
| `demo.rs` | The built-in demo house |
| `model.rs` | `LoxApp3.json` as rooms, categories and typed controls |
| `store.rs` | Live state by state UUID, the event ring, session history, event rate |
| `vm.rs` | View models: what a control looks like now and what each action key does to it |
| `lists.rs` | Derived lists shared by navigation and rendering |
| `keymap.rs`, `palette.rs`, `theme.rs`, `text.rs`, `chart.rs`, `data.rs` | Keymap, palette, themes, text safety, chart math, polled data |
| `ui/`, `widgets/` | Rendering per screen and overlay, and the six widgets |

The UI thread runs a synchronous loop: poll input, drain backend messages, tick, draw (at most 30 fps, only when
dirty). Everything slow runs on a background tokio runtime and reports back over an mpsc channel. HTTP goes through
the blocking `LoxClient` in `spawn_blocking`. The wiring view and config history use `src/logic.rs`, the adapter over
loxone-config.

A guard owns raw mode, the alternate screen, the cursor, mouse capture, bracketed paste and the keyboard flags. It
restores them on exit, error, panic, SIGTERM and SIGHUP. `Ctrl-Z` restores the terminal, suspends, and redraws on
`fg`.

The TUI is behind the cargo feature `tui` (on by default). Preferences go in `~/.lox/tui.yaml`. Per-context state
(pins, mutes, palette history, last screen) goes in `~/.lox/contexts/<name>/tui-state.yaml`.

## Safety

| Risk | Examples | Behavior |
|------|----------|----------|
| None | Lights, dimmers, blinds, music, thermostat target, moods, scenes | Instant |
| Confirm | Doors, gates, the alarm, intercom open, wallbox start, operating mode, door openers wired as push-buttons, controls on the `confirm:` list, a bulk action on more than 10 controls | `y`/`N` dialog, default no |
| Typed | Reboot, firmware update, config upload | Type the context name |

The risk comes from `Action::risk_on` in `src/actions.rs`, shared with `lox mcp`. `--read-only` disables every
action key and shows `READ-ONLY` in the header. PINs use masked input and never enter the palette history. The TUI
never shows passwords or tokens.

## Testing

Tests live in `src/tui/tests.rs` and run against the demo house:

- User journeys as key sequences through `update()`, asserting the effects.
- Every screen and overlay rendered at 60×20, 80×24, 120×36 and 180×50 in `night`, `day` and `mono`. The tests
  check invariants (no panic, intact header, exact frame size) rather than snapshots, so visual polish doesn't churn
  them.
- Edge cases: selection survives re-sort, stale results are dropped, a held `␣` sends one command, a pasted newline
  doesn't submit.
- Scale (2,000 controls, 80 rooms) and hostile text (escape sequences, wide and combining characters).
- A live probe, ignored by default: `LOX_TUI_LIVE=1 cargo test live_probe -- --ignored`.

`LOX_DEMO_HOUR=12.5` pins the demo's time of day, so recordings look the same. `scripts/demo-video/make.sh` renders
the promo video.

## Facts from a real Miniserver

These shaped the design and still hold:

- Meters often have no statistics, and temperature statistics are hourly. The energy "today" chart falls back to
  values seen in this session. Statistics timestamps are local time.
- EFM values arrive only on change. Node power falls back to the node meter's `actual`, then to the EFM totals.
- Gen 2 Miniservers lack some counters (`ctx/s`, `ints/s`, `comints`). System hides counters that aren't reported.
- `sdtest` exercises the SD card, so it is polled at most every 10 minutes.
- Light-circuit switches report `active` as 0/100 instead of 0/1.
- Duplicate names are normal. The palette matches name and room, and `y` always includes `-r <room>`.
- Meters, the EFM and analog states change constantly. They stay out of the Home feed, and changes below display
  precision don't create events.
- The Miniserver log can be months old at the tail. The log view opens on the newest line.
