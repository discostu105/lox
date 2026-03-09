# Command Reference

All commands support `--json` for structured output. Controls can be referenced by name (fuzzy matched), UUID, or alias.

When multiple controls share the same name, disambiguate with `--room` or bracket syntax:
```bash
lox get "Temperatur" --room "Schlafzimmer"
lox get "Temperatur [OG Schlafzimmer]"
```

---

## Setup

```bash
lox setup set --host https://192.168.1.100 --user admin --pass secret
lox setup set --serial YOUR_SERIAL     # enables correct TLS hostname
lox setup show                         # show config (password redacted)
```

### Aliases

```bash
lox alias add wz "1d8af56e-036e-e9ad-ffffed57184a04d2"
lox alias remove wz
lox alias list
```

Then use directly: `lox on wz`

---

## Discovery & Inspection

```bash
lox ls                                 # all controls
lox ls --type Jalousie                 # filter by type
lox ls --room "Wohnzimmer"            # filter by room
lox ls --cat "Beleuchtung"            # filter by category
lox ls --favorites                    # only favorites
lox ls --values                       # include live values (slower)

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
lox mood "Licht Wohnzimmer" plus      # next mood
lox mood "Licht Wohnzimmer" minus     # previous mood
lox mood "Licht Wohnzimmer" off       # turn off (mood 778)
lox mood "Licht Wohnzimmer" 704       # set by mood ID
lox dimmer "Stehlampe" 75             # set dimmer 0-100%
lox color "LED Strip" "#FF0000"       # hex RGB
lox color "LED Strip" "hsv(120,100,100)"  # HSV
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
lox thermostat "Heizung" --temp 22.5               # set comfort temp
lox thermostat "Heizung" --mode auto                # auto|manual|comfort|eco
lox thermostat "Heizung" --override 24 --duration 120  # override for N minutes
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
lox lock "Heizung" --reason "Wartung"  # lock a control
lox unlock "Heizung"
```

---

## Statistics & History

```bash
lox stats                              # controls with statistics enabled
lox history "Temperatur" --month 2025-01
lox history "Temperatur" --day 2025-01-15
lox history "Temperatur" --csv         # CSV output
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
lox set "Sollwert Heizung" 21.5
lox pulse "Taster"
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
lox scene list                         # list all scenes
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
lox autopilot list                     # list all automatic rules
lox autopilot state "Rule Name"        # show when a rule last fired
```

---

## Loxone Config

```bash
lox config download                    # download latest config ZIP via FTP
lox config download --extract          # download + decompress to .Loxone XML
lox config download -o config.zip      # custom output filename
lox config list                        # list all configs on the Miniserver
lox config extract config.zip          # decompress LoxCC → .Loxone XML
lox config extract config.zip -o out.Loxone
lox config upload config.zip --force   # upload to Miniserver (dangerous)
lox config users file.Loxone           # list user accounts from config XML
lox config devices file.Loxone         # list hardware devices (Tree/Air/Network)
lox config diff old.Loxone new.Loxone  # compare two configs (accepts .zip or .Loxone)
```

---

## System

```bash
lox status                             # firmware, PLC state, memory
lox status --energy                    # live energy panel (PV, grid, battery)
lox status --diag                      # CPU, tasks, context switches, SD card
lox status --net                       # network config (IP, MAC, DNS, DHCP, NTP)
lox status --bus                       # CAN bus statistics
lox status --lan                       # LAN packet statistics
lox status --all                       # all diagnostic sections
lox time                               # Miniserver system date/time
lox log                                # system log (admin only)
lox log --lines 100
```

---

## Filesystem

```bash
lox files ls /                         # browse Miniserver filesystem
lox files get /log/def.log             # download a file
lox files get /log/def.log --output local.log
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
lox watch "Temperatur" --interval 5    # custom poll interval in seconds
```

---

## Firmware Update

```bash
lox update check                       # check for updates
lox update install --confirm           # install update
lox reboot --confirm                   # reboot Miniserver
```
