//! The anatomy of Loxone UUIDs, and a deterministic minter.
//!
//! Loxone UUIDs are **not** RFC-4122 UUIDs: the format is
//! `{8 hex}-{4 hex}-{4 hex}-{16 hex}` — the last segment is 8 bytes, not 6.
//! Observed structure (verified against live Miniserver configs):
//!
//! - **Segment 1** (`time`): seconds since the Loxone epoch (2009-01-01),
//!   i.e. the object's creation time.
//! - **Segments 2+3** (`mid`, `seq`): sequence counters; unique per mint.
//! - **Segment 4** (`tail`, 8 bytes): identity of *what* this UUID names:
//!   - `ff ff` + 6 bytes: an **object** (or block state). For objects the
//!     6 bytes identify the minting machine — the PC running Loxone Config,
//!     or the Miniserver's serial number when the Miniserver itself created
//!     the object (app-created autopilot rules, device registrations). For
//!     states it is a per-object random value.
//!   - `<index> ff` + 6 bytes: a **connector** (port). The first byte is the
//!     connector's index within its block; the remaining 6 bytes are a
//!     random per-object value shared by all connectors of that block.
//!   - anything else: reserved/system space — built-in objects such as
//!     operating Modes have fully deterministic UUIDs
//!     (`00000000-0000-0001-1500000000000000`).

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use std::fmt;

/// Unix timestamp of the Loxone epoch, 2009-01-01T00:00:00Z.
pub const LOXONE_EPOCH_UNIX: i64 = 1_230_768_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoxUuid {
    pub time: u32,
    pub mid: u16,
    pub seq: u16,
    pub tail: [u8; 8],
}

/// What segment 4 says this UUID names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailKind {
    /// `ffff` + suffix: an object (minting-machine id) or a block state
    /// (per-object random value) — the two share this shape.
    Object { suffix: [u8; 6] },
    /// `<index>ff` + entity: a connector; `index` is its position within the
    /// block, `entity` is shared by all connectors of the same block.
    Port { index: u8, entity: [u8; 6] },
    /// Reserved/system space (deterministic built-in UUIDs and unknowns).
    Other([u8; 8]),
}

impl LoxUuid {
    pub fn parse(s: &str) -> Result<Self> {
        let err = |msg: &str| Error::Uuid {
            value: s.to_string(),
            msg: msg.to_string(),
        };
        let parts: Vec<&str> = s.split('-').collect();
        let [a, b, c, d] = parts.as_slice() else {
            return Err(err("expected 4 segments"));
        };
        if a.len() != 8 || b.len() != 4 || c.len() != 4 || d.len() != 16 {
            return Err(err("expected segment lengths 8-4-4-16"));
        }
        let time = u32::from_str_radix(a, 16).map_err(|_| err("segment 1 is not hex"))?;
        let mid = u16::from_str_radix(b, 16).map_err(|_| err("segment 2 is not hex"))?;
        let seq = u16::from_str_radix(c, 16).map_err(|_| err("segment 3 is not hex"))?;
        let mut tail = [0u8; 8];
        for (i, byte) in tail.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&d[i * 2..i * 2 + 2], 16)
                .map_err(|_| err("segment 4 is not hex"))?;
        }
        Ok(LoxUuid {
            time,
            mid,
            seq,
            tail,
        })
    }

    pub fn tail_kind(&self) -> TailKind {
        let t = self.tail;
        if t[0] == 0xff && t[1] == 0xff {
            TailKind::Object {
                suffix: t[2..8].try_into().unwrap(),
            }
        } else if t[1] == 0xff {
            TailKind::Port {
                index: t[0],
                entity: t[2..8].try_into().unwrap(),
            }
        } else {
            TailKind::Other(t)
        }
    }

    /// The connector index, if this is a port UUID.
    pub fn connector_index(&self) -> Option<u8> {
        match self.tail_kind() {
            TailKind::Port { index, .. } => Some(index),
            _ => None,
        }
    }

    /// Creation time as a Unix timestamp.
    pub fn created_unix(&self) -> i64 {
        LOXONE_EPOCH_UNIX + i64::from(self.time)
    }
}

impl fmt::Display for LoxUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}-{:04x}-{:04x}-", self.time, self.mid, self.seq)?;
        for b in self.tail {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// Parse a Miniserver serial such as `"504F94A26236"` into 6 bytes.
pub fn parse_serial(s: &str) -> Result<[u8; 6]> {
    let err = || Error::Uuid {
        value: s.to_string(),
        msg: "expected 12 hex digits (Miniserver serial)".to_string(),
    };
    if s.len() != 12 {
        return Err(err());
    }
    let mut out = [0u8; 6];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| err())?;
    }
    Ok(out)
}

/// Derive the 6-byte per-object connector entity from a slug. Deterministic,
/// so re-minting the same slug yields the same port UUID family.
pub fn entity_for_slug(slug: &str) -> [u8; 6] {
    let digest = Sha256::digest(slug.as_bytes());
    digest[..6].try_into().unwrap()
}

/// Deterministic UUID minter.
///
/// All inputs are caller-provided (no clock, no RNG): the same
/// `(machine, time, sequence of calls)` always yields the same UUIDs. In the
/// IR compiler, minted UUIDs are recorded in the lockfile immediately, so
/// determinism across runs comes from the lock; determinism within a run
/// comes from here.
#[derive(Debug, Clone)]
pub struct Minter {
    machine: [u8; 6],
    time: u32,
    counter: u32,
}

impl Minter {
    /// `machine` is the identity stamped into object tails — conventionally
    /// the Miniserver serial (`parse_serial`). `time_unix` is the creation
    /// time recorded in segment 1.
    pub fn new(machine: [u8; 6], time_unix: i64) -> Self {
        let time = (time_unix - LOXONE_EPOCH_UNIX).clamp(0, u32::MAX as i64) as u32;
        Minter {
            machine,
            time,
            counter: 0,
        }
    }

    fn next(&mut self, tail: [u8; 8]) -> LoxUuid {
        let uuid = LoxUuid {
            time: self.time,
            mid: (self.counter >> 16) as u16,
            seq: self.counter as u16,
            tail,
        };
        self.counter += 1;
        uuid
    }

    pub fn mint_object(&mut self) -> LoxUuid {
        let mut tail = [0xff; 8];
        tail[2..8].copy_from_slice(&self.machine);
        self.next(tail)
    }

    pub fn mint_port(&mut self, index: u8, entity: [u8; 6]) -> LoxUuid {
        let mut tail = [0u8; 8];
        tail[0] = index;
        tail[1] = 0xff;
        tail[2..8].copy_from_slice(&entity);
        self.next(tail)
    }

    /// Raise the counter to at least `c` (never lowers it).
    pub fn seed(&mut self, c: u32) {
        self.counter = self.counter.max(c);
    }

    /// Ensure future mints cannot collide with `uuid`: when it carries this
    /// minter's time stamp, advance the counter past its sequence. Uuids
    /// with a different time stamp can never collide and are ignored.
    pub fn avoid(&mut self, uuid: &str) {
        if let Ok(u) = LoxUuid::parse(uuid)
            && u.time == self.time
        {
            let c = ((u.mid as u32) << 16) | u.seq as u32;
            self.counter = self.counter.max(c.saturating_add(1));
        }
    }

    pub fn counter(&self) -> u32 {
        self.counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_roundtrip() {
        for s in [
            "1d844a67-0333-5301-ffffed57184a04d2",
            "1d844a67-0333-52d5-03ff6c7a837b4406",
            "00000000-0000-0001-1500000000000000",
        ] {
            assert_eq!(LoxUuid::parse(s).unwrap().to_string(), s);
        }
    }

    #[test]
    fn tail_classification_matches_live_observations() {
        // Object minted by Loxone Config on a PC:
        let obj = LoxUuid::parse("1d844a67-0333-5301-ffffed57184a04d2").unwrap();
        assert_eq!(
            obj.tail_kind(),
            TailKind::Object {
                suffix: [0xed, 0x57, 0x18, 0x4a, 0x04, 0xd2]
            }
        );
        // Object minted by the Miniserver itself (serial 504F94A26236):
        let ms = LoxUuid::parse("20975553-0369-262a-ffff504f94a26236").unwrap();
        assert_eq!(
            ms.tail_kind(),
            TailKind::Object {
                suffix: parse_serial("504F94A26236").unwrap()
            }
        );
        // Connector #3 (EndUp of an AutoJalousie):
        let port = LoxUuid::parse("1d844a67-0333-52d5-03ff6c7a837b4406").unwrap();
        assert_eq!(port.connector_index(), Some(3));
        // Deterministic system UUID (Mode):
        let mode = LoxUuid::parse("00000000-0000-0001-1500000000000000").unwrap();
        assert!(matches!(mode.tail_kind(), TailKind::Other(_)));
    }

    #[test]
    fn created_time_decodes() {
        // 15ea0aa3 → 2020-08-26 (observed creation date of the live project).
        let u = LoxUuid::parse("15ea0aa3-0372-39b9-ffffed57184a04d2").unwrap();
        let unix = u.created_unix();
        assert!((1_598_000_000..1_599_000_000).contains(&unix));
    }

    #[test]
    fn minter_is_deterministic() {
        let machine = parse_serial("504F94112233").unwrap();
        let mut a = Minter::new(machine, LOXONE_EPOCH_UNIX + 1000);
        let mut b = Minter::new(machine, LOXONE_EPOCH_UNIX + 1000);
        let ua = a.mint_object();
        let ub = b.mint_object();
        assert_eq!(ua, ub);
        assert_ne!(a.mint_object(), ua, "counter must advance");
        let e = entity_for_slug("beschatten");
        assert_eq!(a.mint_port(2, e).connector_index(), Some(2));
        assert_eq!(entity_for_slug("beschatten"), e);
    }
}
