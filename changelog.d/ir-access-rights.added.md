- **Access rights from source** (#140): an `access <code_touch> … end` declaration in a `.lxir` module lists the
  users and groups that may open an NFC Code Touch, by title, each with its access schedule
  (`user "alice": "Always"`). It owns that block's access: compiling grants, reschedules and revokes, writing the
  same `<PRM>` edit Loxone Config makes; Code Touches without a declaration are not touched. Users, groups,
  schedules, tags and codes stay with Loxone Config and the app, and a title that is missing or not unique is an
  error. `lox ir decompile` and `lox ir adopt` lift existing rights into declarations; `lox ir lint` reports
  `no-access-right` for a user whose NFC tag or access code opens no Code Touch.
- lxir reads and generates `permissions.bin`, the program ZIP's table of who may use which object
  (`lxir::permissions`), byte for byte against the files Loxone Config writes.
