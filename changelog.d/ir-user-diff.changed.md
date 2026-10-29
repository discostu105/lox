- `lox config push` no longer refuses when the live config differs from the base only in users, groups,
  credentials or access rights. The push compiles against the live download, so those stay as they are live;
  the report lists them as "kept as live" (`kept_as_live` in `-o json`).
