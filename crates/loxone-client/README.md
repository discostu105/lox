# loxone-client

A Rust client for the [Loxone](https://www.loxone.com) Miniserver (Gen 1 and Gen 2): the HTTP API, token auth,
the WebSocket state stream, FTP backups and statistics. It is the transport of the
[lox](https://github.com/discostu105/lox) CLI, which runs it against real Miniservers every day.

| Module | What |
|:-------|:-----|
| `LoxClient` | HTTP API: commands to controls, their states, the structure file (`LoxApp3.json`, cached 24 h), name resolution (`"Light [Kitchen]"`), retries |
| `token` | Token auth over the WebSocket: RSA key exchange, HMAC credential hash (SHA-1 or SHA-256), `gettoken`; stored and reused |
| `stream` | The state stream: binary event tables (values, texts, daytimers, weather) decoded to `StateEvent`s, keepalives, reconnect-friendly sessions |
| `ftp` | Backups (`/prog/sps_*.zip`), the program upload (`sps_new.zip`), custom changes |
| `statv2`, `stats` | Statistics V2 (energy flow monitor meters), statistics and weather files |
| `sdtest`, `time` | The SD card self-test, Loxone timestamps (seconds since 2009) |

HTTP is blocking (`reqwest::blocking`); the WebSocket parts are async on tokio. TLS is rustls. Certificate
verification is off unless `verify_ssl` is set, because Miniservers use self-signed certificates.

```rust,no_run
use loxone_client::{Connection, LoxClient};

fn main() -> loxone_client::Result<()> {
    let conn = Connection::new("https://192.168.1.77", "admin", "secret");
    let mut lox = LoxClient::new(conn)?;
    let uuid = lox.resolve_with_room("Light", Some("Kitchen"))?;
    lox.send_cmd(&uuid, "on")?;
    Ok(())
}
```

Streaming state changes:

```rust,no_run
use loxone_client::{Connection, stream};

#[tokio::main]
async fn main() -> loxone_client::Result<()> {
    let conn = Connection::new("https://192.168.1.77", "admin", "secret");
    stream::stream_events(&conn, |events| {
        for event in events {
            println!("{event:?}");
        }
        Ok::<_, std::io::Error>(())
    })
    .await
}
```

To keep the token between runs and cache the structure file, set `Connection::token_path` and
`Connection::structure_cache`. To work with the program file itself (`.Loxone`, backup ZIPs), use
[loxone-config](https://crates.io/crates/loxone-config).

## License

Apache-2.0
