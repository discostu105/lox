# Command Reference

All commands support `-o`/`--output` for format selection (`json`, `csv`, `table`) and `-q`/`--quiet` to suppress non-essential output. Controls can be referenced by name (fuzzy matched), UUID, or alias.

When multiple controls share the same name, disambiguate with `-r`/`--room` or bracket syntax:
```bash
lox get "Temperatur" -r "Schlafzimmer"
lox get "Temperatur [OG Schlafzimmer]"
```

Global flags: `-o`/`--output json|csv|table`, `-q`/`--quiet`, `--no-color` (also respects `NO_COLOR` env var), `--no-header`

---

## Setup

```bash
lox setup set --host https://192.168.1.100 --user admin --pass secret
lox setup set --serial YOUR_SERIAL     # enables correct TLS hostname
lox setup set --verify-ssl             # enable cert verification
lox setup set --no-verify-ssl          # disable (default, self-signed)
lox setup show                         # show config (password redacted)
```

All config fields also support env vars: `LOX_HOST`, `LOX_USER`, `LOX_PASS`, `LOX_SERIAL`
```bash
LOX_HOST=https://192.168.1.100 LOX_USER=admin LOX_PASS=secret lox status
```

### Aliases

```bash
lox alias add wz "1d8af56e-036e-e9ad-ffffed57184a04d2"
lox alias remove wz
lox alias ls
```

Then use directly: `lox on wz`

---

## Discovery & Inspection

```bash
lox ls                                 # all controls
lox ls -t Jalousie                     # filter by type
lox ls -r "Wohnzimmer"                 # filter by room
lox ls -c "Beleuchtung"                # filter by category
lox ls -f                              # only favorites
lox ls -v                              # include live values (slower)

lox get "Licht Wohnzimmer"            # full state of one control
lox info "Licht Wohnzimmer"           # detailed: sub-controls, states, moods

lox rooms                             # list all rooms
lox categories                        # list all categories
lox globals                           # global states (operating mode, sunrise, etc.)
lox modes                             # operating modes
lox extensions                        # connected extensions and devices
lox discover                          # find Miniservers on local network (UDP)
lox discover --timeout 5
```

---

## Lights

```bash
lox on "Licht Wohnzimmer"
lox off "Licht Wohnzimmer"
lox light mood "Licht Wohnzimmer" plus      # next mood
lox light mood "Licht Wohnzimmer" minus     # previous mood
lox light mood "Licht Wohnzimmer" off       # turn off (mood 778)
lox light mood "Licht Wohnzimmer" 704       # set by mood ID
lox light dim "Stehlampe" 75                # set dimmer 0-100%
lox light color "LED Strip" "#FF0000"       # hex RGB
lox light color "LED Strip" "hsv(120,100,100)"  # HSV
```

---

## Blinds

```bash
lox blind "Beschattung Süd" up
lox blind "Beschattung Süd" down
lox blind "Beschattung Süd" stop
lox blind "Beschattung Süd" pos 50    # position 0-100%
lox blind "Beschattung Süd" full-up
lox blind "Beschattung Süd" full-down
lox blind "Beschattung Süd" shade     # automatic shading
```

---

## Gates

```bash
lox gate "Garagentor" open
lox gate "Garagentor" close
lox gate "Garagentor" stop
```

---

## Climate

```bash
lox thermostat "Heizung" temp 22.5                 # set comfort temp
lox thermostat "Heizung" mode auto                  # auto|manual|comfort|eco
lox thermostat "Heizung" override 24 120            # override 24°C for 120 min
lox thermostat "Heizung"                            # show current state
lox weather                                         # current weather data
lox weather --forecast                              # 7-day forecast
```

---

## Security

```bash
lox alarm "Alarmanlage" arm            # arm alarm
lox alarm "Alarmanlage" arm --no-motion  # arm without motion detection
lox alarm "Alarmanlage" disarm
lox alarm "Alarmanlage" quit           # acknowledge/silence
lox door "Haustür" lock                # lock a door lock
lox door "Haustür" unlock              # unlock
lox door "Haustür" open                # open (e.g. electric strike)
lox lock "Heizung" --reason "Wartung"  # lock a control (admin)
lox unlock "Heizung"
```

---

## Statistics & History

```bash
lox stats                              # controls with statistics enabled
lox history "Temperatur" --month 2025-01
lox history "Temperatur" --day 2025-01-15
lox history "Temperatur" -o csv        # CSV output
```

---

## Conditions

```bash
lox if "Temperatur Außen" gt 25        # exit 0=true, 1=false
lox if "Schalter" eq 1 && lox on "Licht"
```

Operators: `eq`, `ne`, `gt`, `ge`, `lt`, `le`

---

## Analog / Virtual Inputs

```bash
lox input set "Sollwert Heizung" 21.5
lox input set "Sollwert Heizung" 21.5 -r "Wohnzimmer"  # disambiguate with room
lox input pulse "Taster"
```

---

## Music Server

```bash
lox music play                         # play zone 1
lox music pause 2                      # pause zone 2
lox music stop
lox music volume 50                    # volume 0-100
```

---

## Scenes

```bash
lox run abend                          # run a scene
lox run abend --dry-run                # preview without executing
lox scene ls                           # list all scenes
lox scene show abend                   # print YAML definition
lox scene new abend                    # create empty scene file
```

Scenes are YAML files in `~/.lox/scenes/`:
```yaml
name: Abend
steps:
  - control: "Licht Wohnzimmer"
    cmd: on
  - control: "Beschattung Süd"
    cmd: "pos 70"
    delay_ms: 500
```

---

## Autopilot (Automatic Rules)

```bash
lox autopilot ls                       # list all automatic rules
lox autopilot state "Rule Name"        # show when a rule last fired
```

---

## Loxone Config

```bash
lox config download                    # download latest config ZIP via FTP
lox config download --extract          # download + decompress to .Loxone XML
lox config download --save-as config.zip  # custom output filename
lox config ls                          # list all configs on the Miniserver
lox config extract config.zip          # decompress LoxCC → .Loxone XML
lox config extract config.zip --save-as out.Loxone
lox config upload config.zip --force   # upload to Miniserver (dangerous)
lox config users file.Loxone           # list user accounts from config XML
lox config devices file.Loxone         # list hardware devices (Tree/Air/Network)
lox config diff old.Loxone new.Loxone  # compare two configs (accepts .zip or .Loxone)
```

---

## System

```bash
lox status                             # firmware, PLC state, memory
lox status --diag                      # CPU, tasks, context switches, SD card
lox status --net                       # network config (IP, MAC, DNS, DHCP, NTP)
lox status --bus                       # CAN bus statistics
lox status --lan                       # LAN packet statistics
lox status --all                       # all diagnostic sections
lox time                               # Miniserver system date/time
lox log                                # system log (admin only)
lox log -n 100                         # custom line count
```

---

## Filesystem

```bash
lox files ls /                         # browse Miniserver filesystem
lox files get /log/def.log             # download a file
lox files get /log/def.log --save-as local.log
```

---

## Cache

```bash
lox cache info                         # show cache age and path
lox cache check                        # check if cache is current
lox cache refresh                      # force re-fetch
lox cache clear                        # delete local cache
```

---

## Token Auth

More secure than Basic Auth. Token is valid ~20 days.

```bash
lox token fetch                        # fetch & save token
lox token info                         # show token status
lox token check                        # verify token on Miniserver
lox token refresh                      # extend validity
lox token revoke                       # revoke on Miniserver
lox token clear                        # delete local token file
```

---

## Raw Commands

```bash
lox send <uuid> <command>              # send raw command to a control
lox send <uuid> <command> --secured <hash>  # secured command
lox watch "Temperatur"                 # poll state and print changes (Ctrl+C to stop)
lox watch "Temperatur" -i 5            # custom poll interval in seconds
```

---

## Firmware Update

```bash
lox update check                       # check for updates
lox update install --yes               # install update
lox reboot --yes                       # reboot Miniserver
```

---

## Shell Completions

```bash
lox completions bash                   # generate bash completions
lox completions zsh                    # generate zsh completions
lox completions fish                   # generate fish completions

# Install (bash):
lox completions bash > /etc/bash_completion.d/lox

# Install (zsh):
lox completions zsh > ~/.zfunc/_lox

# Install (fish):
lox completions fish > ~/.config/fish/completions/lox.fish
```
