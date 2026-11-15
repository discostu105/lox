# D135: The project's and the Miniserver's settings from source

Status: accepted (2026-11-15)

## Context

The pages, the hardware and the users came under source control one by one. What stayed with Loxone
Config were the settings of the house itself: where it stands, when and whether the Miniserver updates,
the security switches, and where the Miniserver is mounted. Loxone made the update mode matter in
November 2026: from 1 December, Remote Connect brokers only Miniservers on a current version, and
"never update" switches Remote Connect off altogether.

These settings are attributes of two objects: the project root (`<C Type="Document">`) and the Miniserver
(`<C Type="LoxLIVE">`). Oracle session 94 set each field of the two properties panes in Loxone Config
17.3 and read back what a save writes.

## Decision

**The project and the Miniserver are externs whose settings are claims**, like a device's
([D65](0065-physical-io-settings.md)): `haus.Timezone = "Europe/Vienna"` on
`extern haus = Document(uuid: …)`. The original goes to the lock and comes back when the statement
leaves the source.

- **Names.** The attribute's name where it is the plain word (`Street`, `Timezone`, `Currency`),
  the pane's word where it is a code (`Country` → `Ctry=`, `Altitude` → `SL=`, `LoxoneLog` →
  `CrashL=`), as [D95](0095-block-settings.md) names block settings.
- **Values.** `Updates` takes `"auto"`, `"confirm"` or `"never"` and writes `AUE=` and `AUM=`
  together; `"auto"` leaves a 17.2 save's automatic (`AUE="true"`, no `AUM=`) as it is. `UpdateTime` takes a time of day and writes minutes after midnight. The switches take
  `on`/`off` and write what a save writes for each, which for most is no attribute.
- **Rewritten, not added.** The text settings exist in every 17.x save, so a missing one means an old
  or synthetic project, and the compile refuses it. The settings a save adds when set (`SL=`, the
  switches, `AUM=`) are added in the order a 17.3 save writes them.
- **Not claimable:** the identity and bookkeeping (`U`, `ConfigVersion`, `Date`, `NumO`, `APPKEY`,
  `APPID`), the program cycle (`SPSfreq`), the display units (codes whose words are unknown),
  `DeviceTelemetry` (no pane shows it), and the addresses Loxone Config connects to (`IntAddr`,
  `ExtAddr`), which change nothing on the Miniserver. The Miniserver's name stays a `.title` claim.
- **The drift fingerprint v14 includes the claimable attributes** of both objects. A lock recorded with
  v13 still reads as in sync.

## Consequences

- A push can switch the automatic updates off. `"never"` also switches Remote Connect off, so the
  language reference says so where it lists the value.
- `adopt` does not write these claims yet: the source states them by hand.

## Evidence

Oracle session 94 ([oracle-sessions.md](../oracle-sessions.md)) and the table in
[loxone-format.md](../loxone-format.md) (*Project and Miniserver settings*).
