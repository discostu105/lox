- **`loxl` / `lox logic`: a `page` statement governs only its own file.** In a directory module, blocks
  above a file's first `page` take the default page instead of the page the previous file named, so
  renaming or adding a file no longer moves blocks between pages. A single-file module does not change
  - `loxl lint` reports `default-page` for a file whose blocks come before its first `page` while other
    files name their page. Add the `page` line the file relied on
