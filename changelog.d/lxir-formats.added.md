- **lxir**: every command that reads a config (`lxir check`, `compile`, `diff`, `decompile`, …) also takes a
  Miniserver backup ZIP; the library gains `lxir::archive` (feature `archive`, on by default): the LoxCC codec,
  extracting the program file and staging an upload ZIP, moved over from lox
