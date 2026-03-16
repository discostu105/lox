# PR #60 Review: Production Readiness Assessment

**Verdict: Not production-ready yet.** 1 critical bug, 2 medium issues, and minor items to address.

---

## Critical: UTF-8 panic in `log_body`

`src/client.rs` — the `log_body` function slices at byte 500:

```rust
fn log_body(body: &str) {
    if body.len() > 500 {
        eprintln!("  body: {}… ({} bytes total)", &body[..500], body.len());
    }
```

If the 500th byte falls in the middle of a multi-byte UTF-8 character (common with German
Umlauts like ä, ö, ü in Loxone control names), this **panics at runtime**. Fix by finding
a valid char boundary:

```rust
let end = body.char_indices()
    .take_while(|(i, _)| *i < 500)
    .last()
    .map_or(0, |(i, c)| i + c.len_utf8());
eprintln!("  body: {}… ({} bytes total)", &body[..end], body.len());
```

## Medium: Same-origin check ignores port

`same_origin_redirect_policy` compares only `host_str()`, not the port. A redirect from
`:8080` to `:443` on the same host would be silently followed. Origin in web security
includes scheme + host + port.

## Medium: Host parsing failure silently allows all redirects

`reqwest::Url::parse(&configured_host)` fails on bare hostnames like `192.168.1.5` (no
scheme). When it fails, the `and_then` chain returns `None`, `filter` sees no mismatch,
and **all redirects are silently allowed** — defeating the entire protection.

## Minor

1. `redact_url` only catches first occurrence of duplicate query params
2. `redact_url` is `pub` but only used within `client.rs` — should be `pub(crate)` or private
3. Verbose logging asymmetry in retries: request URL logged once before loop, status logged inside
4. `CacheCmd::Version` path likely also builds a `Client` without the redirect policy

## What's good

- Cross-origin redirect concept is the right fix for Gen 2 Miniserver behavior
- Credential redaction in verbose logging is well done
- Solid test coverage (redirect blocking, same-origin pass-through, redaction)
- `AtomicU8` for global verbosity is clean and thread-safe
- Idiomatic `clap::ArgAction::Count` for `-v`/`-vv`

## Recommendation

Fix the critical UTF-8 panic and the silent bypass on bare hostnames. The rest can be follow-ups.
