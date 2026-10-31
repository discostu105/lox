# D9: Transport is out of scope

Status: accepted (2026-08-24)

## Decision

The library is pure: bytes in, bytes out. FTP, credentials and every network access live in lox
(loxone-client). Backup ZIPs and the LoxCC codec are file formats, so they live in this crate behind the
`archive` feature.
