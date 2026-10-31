# Miniserver API survey (research, March 2026)

Research notes from 2026-03-08/09, collected while planning lox. They have not been checked again since,
so verify a fact before you build on it. What lox implements is in [COMMANDS.md](../../COMMANDS.md) and
[architecture.md](../architecture.md). The sources are listed in [references.json](../references.json).

## Not used by lox yet

| What | Endpoint | Notes |
|---|---|---|
| Task recorder | via `/jdev/sps/io/` | Commands the Miniserver runs at set times. There are no state updates, so you have to poll. |
| UPnP description | `GET /upnp.xml` | Device description. |
| Public IP | `dns.loxonecloud.com/?getip&snr={serial}&json=true` | Loxone Cloud DNS. |
| Mail | `mail.loxonecloud.com/sendmail/{serial}` | Loxone Mailer Service. |
| Push | `push.loxonecloud.com/v1/push` | Push notifications, signed with HMAC-SHA1. |
| Phone call | `caller.loxone.com/cgi-bin/loxlive/call.pl` | Text-to-speech call. |
| Auto-update | `GET /jdev/sys/autoupdate` | Starts the automatic update. lox uses `updatetolatestrelease`. |
| Extensions, Air/Tree devices | extension and device commands | Need full access. Address a device by serial or name. |

## Structure file fields

`LoxApp3.json` keys lox doesn't read (checked 2026-11-01): `weatherServer` (coordinates, elevation),
`messageCenter` and `times`. The Structure File document is the reference.

## Files on the Miniserver

| Path (FTP, port 21) | Content |
|---|---|
| `/prog/sps_<version>_<YYYYMMDDHHMMSS>.zip` | Program backups. The newest is the current program. |
| `/data/LoxApp3.json` | Structure file |
| `/data/weatheru.bin` | Weather cache |
| `/log/`, `/stats/`, `/web/` | Logs, statistics, web interface |

- FTP reads and writes with an admin user. It sends credentials in plain text, so use it only on the LAN.
- `GET /dev/fsget/<path>` reads a file. It doesn't work under command encryption.
- HTTP has no endpoint to list a directory (`fslist`) or to upload a file (`fsput`).
- `sps0.LoxCC` starts with the magic `0xaabbccee`, then the compressed size (4 bytes), then two header
  words: the approximate uncompressed length and a value that may be a checksum. The payload is LZSS-style.
  [loxone-format.md](../../crates/loxone-config/docs/loxone-format.md) and `archive.rs` have the codec.
