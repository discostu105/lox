# D137: Plugins in the Network Periphery

Status: accepted (2026-11-15)

## Context

Loxone Config 17 adds integrations as plugins: MCP Server, Matter, HomeKit, Exosphere and others, each
a package Loxone Config downloads and installs on the Miniserver. In the config a plugin is an object
in the *Network Periphery* folder with its settings in `<SET>`. Until now the source could not name one,
so enabling MCP Server or Matter, or changing their URLs, happened only in Loxone Config.

## Decision

**A plugin is a hardware declaration, `Plugin(title, …)`.**

- **`uuid:` takes over a plugin** Loxone Config made. **`gid:` creates one** and the lockfile keeps its
  UUID, as `kind:` does for a Tree device ([D102](0102-created-tree-devices.md)): a base without it gets
  it back under that UUID, and deleting it takes `removed <slug>`.
- **Creating is limited to MCP Server and Matter**, the two packages checked in an oracle session. Any
  other is declared by `uuid:` once Loxone Config has added it.
- **The settings are the package's parameters**, checked against a table of the 21 manifests Loxone
  Config 17.3 ships (embedded, read offline). The source key is the parameter id with `-` as `_`. A
  setting is written every compile, as a device setting is, and one the source does not name stays as
  it is. Values follow what a save writes: a tick box is `v="true"` or no `v=`, an empty text no `v=`.
- **`maxCountPerMiniserver` is honoured**: creating one more than a Miniserver takes is an error that
  names the plugin the config already holds.
- **`adopt --uuid`** writes the declaration with the parameters the package shows, so adopting changes
  nothing.

## Consequences

- The compile writes the object only. Installing the package on the Miniserver stays with Loxone Config
  or the push.
- The semantic diff and the drift fingerprint (v15) compare `<SET>`, one change per parameter
  (`param <id>`). `v="false"` and an empty `v=` read as no value, since a save drops them. A lock with
  the v14 form still reads as in sync.
- The first save of a created plugin adds `Desc=` and `NDOC=` from the package in the GUI's language,
  so that save is not a byte no-op. The compile does not write them.
- A plugin recreated under its locked UUID gets new UUIDs for its online status, as a recreated Tree
  device's I/O does.
- The online status's output is not declared; reading it stays with an extern.

## Evidence

Oracle session 96 ([oracle-sessions.md](../oracle-sessions.md)) and *Plugins in the Network Periphery*
in [loxone-format.md](../loxone-format.md).
