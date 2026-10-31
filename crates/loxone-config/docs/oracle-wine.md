# The save oracle: Loxone Config under Wine

Loxone Config saves projects offline, without a Miniserver. Under Wine on Linux it becomes a scriptable
**oracle**: open a compiled config, save it, and diff. Every difference is a compiler bug or a new fact about
the format. The log of every run is [oracle-sessions.md](oracle-sessions.md).

## What the oracle runs established

- Loxone Config opens Lox Logic output and saves it with an empty semantic diff. Its own saves round-trip
  byte for byte through the `xml` layer.
- What every save changes (counters, dates, wire order, `WF`, geometry): [loxone-format.md, *What a save
  changes*](loxone-format.md#what-a-save-changes).
- A save writes exactly the type's connector set, in its order; gates do not grow
  ([D8](decisions/0008-fixed-gate-inputs.md), [*UUID anatomy*](loxone-format.md#uuid-anatomy)).
- A save adds the GUI-owned children a minted block lacks, and the next compile carries them
  ([D19](decisions/0019-gui-residue-carried-forward.md), [*What a first save adds*](loxone-format.md#what-a-first-save-adds)).
- `<Co>` attributes are written `K, Def, Nc, U`; `FLG=` on wires is inert stored state
  ([*Connectors and wires*](loxone-format.md#connectors-and-wires-co--in)).
- Minted refs survive; their titles follow the target, a missing feed wire is drawn by the save
  ([D33](decisions/0033-minted-mirrors.md), [D43](decisions/0043-title-claim-carries-refs.md),
  [*Refs*](loxone-format.md#refs-c-typeinputrefoutputref-ref)).
- Repaired numeric references stay raw ([D44](decisions/0044-loxl-repair.md),
  [*Not actually XML*](loxone-format.md#not-actually-xml)).
- Loxone Config 17.2 writes every block at `V="178"` ([D46](decisions/0046-block-version.md),
  [*Objects*](loxone-format.md#objects-c)).
- Push notifications, rules, pages, moods, modes, calendar entries, devices and their settings: each has its
  section in [loxone-format.md](loxone-format.md), with the decision that writes it.
- A save keeps a wire drawn in the wrong direction, so a save never proves a connector's direction
  ([connector-db.md](connector-db.md)).
- Canvas labels of input keys: [loxone-format.md](loxone-format.md#canvas-labels-of-input-keys).

## The rig, one command

Everything below is wrapped in [`scripts/oracle.sh`](../scripts/oracle.sh):

```sh
scripts/oracle.sh run compiled.Loxone --out saved.Loxone   # open → save → diff
```

`run` does all of it:

1. Starts Xvfb and seeds the prefix's ProgramData from the installed package, as the packaged launcher does.
2. Opens a **copy** of the file in the isolated wineprefix.
3. Answers the recovery dialog with No. It stops when a fresh minidump shows that Loxone Config crashed.
4. Unmaps the news overlay and saves with Ctrl+S (polling the md5).
5. Runs `loxl diff` from the compiled file to the saved one, and tears the rig down.

`--keep` leaves the rig warm. The subcommands `up`, `open`, `save`, `shot`, `status` and `down` drive the
pieces for interactive sessions. Paths come from the environment: `ORACLE_PREFIX`, `ORACLE_EXE`,
`ORACLE_DISPLAY`, `ORACLE_DIR`, `LOXL`. The script refuses a prefix whose wineserver already serves another
display, since that would open the file as a tab in the desktop instance.

## The rig

- Prefix: `~/.local/share/loxone-config/wine`, exe under
  `drive_c/Program Files (x86)/Loxone/LoxoneConfig/`. `LoxoneFormat.exe`
  is an SD-card formatter, not a CLI. There is no headless mode; the GUI
  must be driven.
- Open a file directly (spawns/reuses the GUI):
  `wine LoxoneConfig.exe 'Z:\path\to\copy.Loxone'` — always on a **copy**;
  never open the live project file. Opening never contacts the
  Miniserver; the connect action is a separate explicit button that
  automation must never touch.
- An auto-backup recovery dialog ("Soll diese wiederhergestellt
  werden?") may appear for a project identity that was open before —
  answer **No** (`Alt+N`) so the file opens as compiled.
- **ProgramData must match the installed version.** Loxone Config reads its icon library, block images and
  templates from `ProgramData/Loxone/Loxone Config <version>`. The packaged launcher
  (`/usr/bin/loxone-config`) copies them from `/opt/loxone-config-bin/programdata` into *its* prefix whenever
  the package version changes, stamped in `.loxone-config-bin-version`. The oracle runs the exe directly, so
  `oracle.sh up` seeds its prefix the same way, with the same stamp. Without it, loading takes 10+ minutes
  at full CPU and ends in "Die Icon Library konnte nicht geladen werden", and a `Ctrl+S` sent meanwhile never
  saves. With it, a full house-config open+save takes 17 s.
- Loxone Config's message boxes carry the main window's caption
  (`Loxone Config`, ~84 px high). `oracle.sh` tells the main window
  apart by height, and a failed save says when a message box is up.
- **Keyboard injection works** on KDE Wayland: activate the window via a
  KWin script (`workspace.activeWindow = w` over qdbus, matching caption
  `Loxone Config`), then send X11 XTEST key events (python-xlib) —
  XWayland delivers them to the focused Wine window. `Ctrl+S` saves;
  a freshly opened file is already dirty (schema migration), so the save
  always writes.
- **Pointer injection does not work** via XTEST under KWin: fake motion
  is ignored and clicks land at the physical cursor. The fix is the
  **headless rig**: run
  `Xvfb :5 -screen 0 2560x1440x24 -fbdir <dir> +extension GLX +render
  -noreset`, launch with `DISPLAY=:5` and `LIBGL_ALWAYS_SOFTWARE=1` —
  there `xdotool` has full native pointer *and* keyboard control
  (block selection, wire drags with mid-drag screenshots, menus).
  Screenshots come straight off the framebuffer:
  `magick xwd:<dir>/Xvfb_screen0 shot.png`.
- Xvfb quirks: the QtWebEngine news panel renders as a white overlay
  child window (~630×500) — find it via `xdotool search --name ".*"` +
  geometry and remove it with `xdotool windowunmap <id>`. Wine popup
  menus/pickers ignore Escape and outside clicks under Xvfb; unmap those
  the same way. Trust `xdotool search --onlyvisible` over screenshots
  (stale repaints), and trust the saved XML over the canvas.
- Project bookkeeping: opening a file rewrites
  `drive_c/users/<user>/Documents/Loxone/Loxone Config/Projects/Projects.json`
  (maps project → file path + Miniserver address) — useful to verify
  *which* file an instance actually has open, since window titles don't
  show paths.

- **Always compile onto a real base config.** Loxone Config crashes (SIGSEGV) on load of every synthetic
  config, `examples/out` included, in 17.1 and 17.2.8.28, right after building the document's docking
  panes. A real config opens in the same prefix. The minidump lands in the prefix's
  `AppData/Local/Temp/Loxone/Loxone Config/Dumps/`. The crash box is captioned `LoxoneConfig`, like the
  recovery dialog, so `oracle.sh` watches the dumps folder and stops with that diagnosis. A redacted
  library config (`examples/library/`) or Loxone's factory default
  `programdata/SDcard/prog/Default2.Loxone` works as a base.
- **Driving the GUI** on the headless rig:
  - Close a modal popup (the permissions dialog) with Alt+F4. Unmapping it with xdotool leaves the GUI
    modal, and Ctrl+S then does nothing.
  - Drag a tree item onto a page slowly: mouse down, a small move, a pause of about 0.8 s, moves in steps
    of 0.15 s, a pause, mouse up. A fast drag drops nothing.
  - `xdotool type` mangles non-ASCII (`ä` arrives as `°`). Send keysyms (`xdotool key adiaeresis`).
  - Use a private `ORACLE_DIR` and `ORACLE_DISPLAY` when another session may run the rig. A second
    `oracle.sh` on the same display tears down the first one's Xvfb.
  - The canvas scrolls only to the page extent computed on load. Blocks compiled below the last used row
    save fine but cannot be scrolled into view; read the XML instead.
- **Loxone Config's simulation** (*Testen* › *Simulation starten*) works offline as a behavior oracle. It is
  Loxone's model of the blocks, not a Miniserver. A digital virtual input toggles with the switch icon on its
  box, and every connector shows its live value. Show hidden outputs with the block's ⊕ dialog before
  starting. The compiler mints no `InputRef` boxes for virtual inputs, so drag them from the periphery tree
  onto the canvas and save once. A `Formula` over digital inputs drives an analog input; a `Formula` (`I1`)
  on an output shows its value as a number.
