# D89: What dead-outputs counts as used, and `# lint: allow`

Status: accepted (2026-11-01)

## Context

On the house config `loxl lint` reported 216 `dead-outputs` and 15 `tile-dead-command` findings.
The few that pointed at real leftovers were lost among them. Sorted by cause, the 216 were:

| Findings | What they were | Used? |
|---:|---|---|
| 160 | inputs of Modbus devices, HTTP and UDP commands no wire reads: a device brings all its registers | not read, but not a bug |
| 37 | meters, an energy flow monitor, three Modbus sensors: shown in the app, recording statistics | yes, by the app |
| 8 | operating modes and calendar entries | yes, by mode number |
| 2 | a frost warning (`OutputRefLM`) and the `Or` that feeds it | yes, it sends a push |
| 1 | a Modbus actuator a formula drives: it has no outputs to read | yes |
| 8 | push buttons and virtual inputs that switch nothing, two Touch Pure Flex blocks linked to no device, a room controller that drives nothing, an undriven Modbus actuator | real findings |

The 15 tile findings were five dead commands on each of three Touch Pure Flex devices with the same
cause: no light controller linked.

A clean house config also needs a way to keep a deliberate finding: an unused register list kept
for later, a value only Home Assistant reads.

## Decision

**Used is what something reads.** Besides a wire, `dead-outputs` counts these as uses, each a
documented format fact:

- a value the app shows: `<IoData Visu="true">` with `<Display StateOnly="true">`, the info-only
  controls of `LoxAPP3.json` (loxone-format.md, *Entries by block type*);
- a statistic the block records (`StatsType=`);
- a node of an energy flow monitor (`<nd ctrlUuid=>`);
- a `<LoggerMailer>` child: the block logs, mails or notifies, and whatever feeds it is used;
- modes and calendar entries always: schedules and rules read them by mode number;
- a virtual output and its commands are judged by what drives them: they have no outputs.

A switch or a controller in the app (`Visu="true"` without the info-only display) is still flagged.
It is in the app to drive something, and one that drives nothing is the bug the lint is for.

**A device's unused inputs are one finding.** Two or more inputs of one device (Modbus device,
virtual HTTP or UDP input) that feed nothing are reported once on the device, with the count and
the names. The device is where the decision is made: use the registers, delete them, or keep them.

**One tile finding per device and command type.** The dead commands of one abbreviation on one
device share their cause and are one finding listing the tiles.

**`# lint: allow <kind>` on the line above.** A whole-line comment `# lint: allow` with one or more
kind labels keeps those findings of the declaration right below it out of the report: a block,
extern, instance (all its blocks), device (all its inputs), constant, template, signal or
`defaults`. Inside a template body it covers that block in every instance. Text after the kinds is
the reason. `loxl fmt` keeps the comment right above its statement. An allow that covers nothing,
names no kind or stands above no declaration is itself a finding (`unused-allow`), so a stale allow
cannot hide a later bug. Without a project the compiled-config kinds are not judged.

After the change the house config has 19 `dead-outputs` and 3 `tile-dead-command` findings, each a
real leftover or a deliberate case for an allow.

## Rejected

- **Exempt everything the app shows.** It would hide a light controller or a switch that drives
  nothing, the most useful finding.
- **A trailing comment (`x = And(…)  # lint: allow …`).** Decompile writes trailing comments on
  externs (the object's name), and a multi-line call has no single line for it. One spelling: the
  line above.
- **A block attribute or argument (`lint: off`).** Lint is advisory and never changes the compiled
  config; an argument would look like a setting.
- **Exempt unread device inputs.** An HTTP input polled for nothing is worth a look. Grouping makes
  it one line, an allow on the device makes it zero.
- **A config file listing allowed slugs.** It drifts from the source on a rename; a comment moves
  with its statement and `loxl rename` keeps it.
