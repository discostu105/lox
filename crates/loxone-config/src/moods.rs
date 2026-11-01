//! LightController2 moods (*Stimmungen*): the read model and the
//! value codec shared by `moods` claims (`logic::moods`) and the semantic
//! diff.
//!
//! A controller keeps its moods in a `<LightscenesC Outputs= Num= FC=>`
//! child: one `<LightsceneC Name= UUID= SID= CID= Outputs= Q1=… Qn=>`
//! per mood ([loxone-format.md](../docs/loxone-format.md)). `Q<n>` is the
//! value of lighting circuit `n` — the circuit behind output `AQ<n>`.
//! Its top four bits are a property of the *circuit*, not of the mood:
//! across 133 real configs (648 circuit columns)
//! no circuit carries two different prefixes. The low 28 bits are the
//! value, in a decimal packing that follows the prefix:
//!
//! | prefix | observed values | meaning |
//! |---|---|---|
//! | `0` | `0`, `1`, `50`, `100` | switch (`0`/`1`) or dimmer (percent) |
//! | `1` | `0` … `100` | dimmer (percent) |
//! | `4` | `BBBGGGRRR` | RGB, percent per channel |
//! | `5`, `6` | `BBBGGGRRR`, `20BBBTTTT` | RGB, or tunable white: brightness percent and Kelvin |
//!
//! So the compiler reads a circuit's kind off the controller's existing
//! moods (every saved controller has at least the built-in off mood,
//! `SID="778"`) and refuses a value that prefix has never been seen to
//! carry — refuse, never guess. Only a `Circuit<n>:` parameter in source
//! sets a kind, and only switch (0) or dimmer (1), the two the mood
//! editor was seen to write.

use crate::doc::{LoxoneDoc, ObjectSummary};
use crate::xml::Element;
use std::collections::BTreeMap;
use std::fmt;

/// The built-in off mood ("Aus"/"Off"): always present, read-only, every
/// circuit off.
pub const OFF_SID: &str = "778";

/// The value of one circuit in a mood, in source terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoodValue {
    /// A switch circuit on (`1`).
    On,
    /// Off, on any circuit (`0`, the circuit's prefix kept).
    Off,
    /// A dimmer at this percentage (`0` … `100`).
    Pct(u8),
    /// An RGB colour, percent per channel.
    Rgb(u8, u8, u8),
    /// Tunable white: brightness percent and colour temperature in Kelvin.
    Temp(u8, u16),
    /// The stored `Q<n>` value verbatim — the escape hatch for a packing
    /// Lox Logic cannot name.
    Raw(u32),
}

impl fmt::Display for MoodValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoodValue::On => f.write_str("on"),
            MoodValue::Off => f.write_str("off"),
            MoodValue::Pct(p) => write!(f, "{p}%"),
            MoodValue::Rgb(r, g, b) => write!(f, "rgb({r}, {g}, {b})"),
            MoodValue::Temp(b, k) => write!(f, "temp({b}%, {k}K)"),
            MoodValue::Raw(q) => write!(f, "{q}"),
        }
    }
}

/// Tunable-white values are `200000000 + brightness·10000 + kelvin`.
const TEMP_BASE: u32 = 200_000_000;

/// The circuit kind a prefix stands for, in the words of an error.
fn kind(prefix: u32) -> &'static str {
    match prefix {
        0 => "a switch or dimmer circuit",
        1 => "a dimmer circuit",
        4 => "an RGB circuit",
        5 | 6 => "a colour circuit (RGB or tunable white)",
        _ => "a circuit of a kind Lox Logic does not know",
    }
}

/// The stored `Q<n>` for `value` on a circuit with this prefix, or why
/// the circuit cannot take it.
pub fn encode(prefix: u32, value: MoodValue) -> Result<u32, String> {
    let packed = |payload: u32| (prefix << 28) | payload;
    let refuse = |what: &str| {
        Err(format!(
            "{what} does not fit {}: {}",
            kind(prefix),
            match prefix {
                0 => "it takes `on`, `off` or a percentage",
                1 => "it takes `off` or a percentage",
                4 => "it takes `off` or `rgb(…)`",
                5 | 6 => "it takes `off`, `rgb(…)` or `temp(…)`",
                _ => "give the stored value as a plain number",
            }
        ))
    };
    match value {
        MoodValue::Raw(q) => Ok(q),
        MoodValue::Off => Ok(packed(0)),
        MoodValue::On if prefix == 0 => Ok(1),
        MoodValue::On => refuse("`on`"),
        MoodValue::Pct(p) if prefix <= 1 => Ok(packed(p.into())),
        MoodValue::Pct(p) => refuse(&format!("`{p}%`")),
        MoodValue::Rgb(r, g, b) if matches!(prefix, 4..=6) => Ok(packed(
            u32::from(r) + 1000 * u32::from(g) + 1_000_000 * u32::from(b),
        )),
        v @ MoodValue::Rgb(..) => refuse(&format!("`{v}`")),
        MoodValue::Temp(b, k) if matches!(prefix, 5 | 6) => {
            Ok(packed(TEMP_BASE + 10_000 * u32::from(b) + u32::from(k)))
        }
        v @ MoodValue::Temp(..) => refuse(&format!("`{v}`")),
    }
}

/// A stored `Q<n>` in source terms. Every value decodes to something that
/// encodes back to it: what fits no known packing stays [`MoodValue::Raw`].
pub fn decode(q: u32) -> MoodValue {
    let (prefix, payload) = (q >> 28, q & 0x0fff_ffff);
    let pct = |p: u32| u8::try_from(p).ok().filter(|p| *p <= 100);
    let value = match prefix {
        _ if payload == 0 && prefix != 0 => MoodValue::Off,
        0 => match payload {
            0 => MoodValue::Off,
            1 => MoodValue::On,
            p => pct(p).map_or(MoodValue::Raw(q), MoodValue::Pct),
        },
        1 => pct(payload).map_or(MoodValue::Raw(q), MoodValue::Pct),
        4..=6 if payload >= TEMP_BASE && matches!(prefix, 5 | 6) => {
            let rest = payload - TEMP_BASE;
            match (pct(rest / 10_000), u16::try_from(rest % 10_000)) {
                (Some(b), Ok(k)) if k > 0 => MoodValue::Temp(b, k),
                _ => MoodValue::Raw(q),
            }
        }
        4..=6 => match (
            pct(payload % 1000),
            pct(payload / 1000 % 1000),
            pct(payload / 1_000_000),
        ) {
            (Some(r), Some(g), Some(b)) => MoodValue::Rgb(r, g, b),
            _ => MoodValue::Raw(q),
        },
        _ => MoodValue::Raw(q),
    };
    // Raw stays raw; anything named must round-trip exactly.
    match value {
        MoodValue::Raw(_) => value,
        v if encode(prefix, v) == Ok(q) => v,
        _ => MoodValue::Raw(q),
    }
}

/// One `<LightsceneC>` as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mood {
    pub name: String,
    pub sid: Option<String>,
    pub cid: Option<String>,
    /// `Q1` … `Q<outputs>`; a missing or unreadable attribute reads 0.
    pub qs: Vec<u32>,
}

impl Mood {
    pub fn is_off(&self) -> bool {
        self.sid.as_deref() == Some(OFF_SID)
    }
}

/// A controller's `<LightscenesC>`, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moods {
    /// The circuit count (`Outputs=`; 18 on every observed controller).
    pub outputs: usize,
    pub moods: Vec<Mood>,
}

impl Moods {
    /// The `<LightscenesC>` of a LightController2 element, if it has one —
    /// a controller Lox Logic minted has none until its first Loxone Config
    /// save.
    pub fn read(el: &Element) -> Option<Moods> {
        let scenes = el.child_elements().find(|c| c.name == "LightscenesC")?;
        let outputs = scenes
            .attr("Outputs")
            .and_then(|o| o.parse().ok())
            .unwrap_or(0);
        let moods = scenes
            .child_elements()
            .filter(|c| c.name == "LightsceneC")
            .map(|m| Mood {
                name: m.attr_decoded("Name").unwrap_or_default().into_owned(),
                sid: m.attr("SID").map(String::from),
                cid: m.attr("CID").map(String::from),
                qs: (1..=outputs)
                    .map(|n| {
                        m.attr(&format!("Q{n}"))
                            .and_then(|q| q.parse().ok())
                            .unwrap_or(0)
                    })
                    .collect(),
            })
            .collect();
        Some(Moods { outputs, moods })
    }

    /// Each circuit's prefix (index 0 = circuit 1), read off the moods.
    /// Two moods disagreeing on a circuit's prefix is a config Lox Logic has
    /// never seen, and refused.
    pub fn prefixes(&self) -> Result<Vec<u32>, String> {
        let mut out: Vec<Option<u32>> = vec![None; self.outputs];
        for m in &self.moods {
            for (i, q) in m.qs.iter().enumerate() {
                let p = q >> 28;
                match out[i] {
                    Some(seen) if seen != p => {
                        return Err(format!(
                            "circuit AQ{} has two value kinds in its moods (prefixes {seen} and {p}), which \
                             Lox Logic does not support. Edit its moods in Loxone Config.",
                            i + 1
                        ));
                    }
                    _ => out[i] = Some(p),
                }
            }
        }
        Ok(out.into_iter().map(|p| p.unwrap_or(0)).collect())
    }
}

/// One mood's circuit values in source terms: `all <v>` for the value
/// most circuits share (when that is more than half of them and not
/// `off`), then every circuit that differs from it — `all on, AQ3 off`,
/// `AQ1 temp(80%, 2700K)`. Circuits left out are off; a mood with
/// every circuit off reads `all off`. As pairs of circuit number
/// (`None`: `all`) and value — the settings of a `moods` line, which
/// compile back to exactly `qs`.
pub fn settings(qs: &[u32]) -> Vec<(Option<usize>, MoodValue)> {
    let values: Vec<MoodValue> = qs.iter().map(|&q| decode(q)).collect();
    let mut counts: Vec<(MoodValue, usize)> = Vec::new();
    for v in &values {
        match counts.iter_mut().find(|(c, _)| c == v) {
            Some((_, n)) => *n += 1,
            None => counts.push((*v, 1)),
        }
    }
    let base = counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .filter(|(v, n)| *v != MoodValue::Off && *n * 2 > values.len())
        .map(|(v, _)| *v);
    let mut out: Vec<(Option<usize>, MoodValue)> = base.map(|b| (None, b)).into_iter().collect();
    let default = base.unwrap_or(MoodValue::Off);
    for (i, v) in values.iter().enumerate() {
        if *v != default {
            out.push((Some(i + 1), *v));
        }
    }
    if out.is_empty() {
        out.push((None, MoodValue::Off));
    }
    out
}

/// [`settings`] as text: `all on, AQ2 off`.
pub fn render(qs: &[u32]) -> String {
    settings(qs)
        .iter()
        .map(|(c, v)| match c {
            Some(n) => format!("AQ{n} {v}"),
            None => format!("all {v}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The moods of a LightController2 for the diff and the fingerprint,
/// keyed `mood "<name>"`, the value its circuits in source terms
/// ([`render`]) — plus `moods` with the order of the names, so a
/// reordering shows too. Not a controller, or one without moods: empty.
pub fn mood_fields(doc: &LoxoneDoc, o: &ObjectSummary) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if o.block_type != "LightController2" {
        return out;
    }
    let Some(moods) = doc.element_at(&o.path).and_then(Moods::read) else {
        return out;
    };
    let quoted = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    for m in &moods.moods {
        out.insert(format!("mood {}", quoted(&m.name)), render(&m.qs));
    }
    let order: Vec<String> = moods.moods.iter().map(|m| quoted(&m.name)).collect();
    out.insert("moods".into(), order.join(", "));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn house_and_corpus_values_decode() {
        // Values from the house config and the corpus (loxone-format.md).
        for (q, want) in [
            (0, "off"),
            (1, "on"),
            (50, "50%"),
            (0x1000_0050, "80%"),
            (0x5000_0000, "off"),
            (1_543_179_980, "temp(100%, 2700K)"),
            (1_423_277_378, "rgb(98, 100, 81)"),
            (1_811_487_473, "temp(87%, 4737K)"),
            (0x4000_0000 + 100_100_100, "rgb(100, 100, 100)"),
            (1_610_622_736, "rgb(0, 10, 0)"),
        ] {
            assert_eq!(decode(q).to_string(), want, "{q:#x}");
            assert_eq!(encode(q >> 28, decode(q)), Ok(q), "{q:#x} round-trips");
        }
        // Out of every packing: kept verbatim.
        assert_eq!(
            decode(0x6000_0000 + 150_000_000),
            MoodValue::Raw(0x6000_0000 + 150_000_000)
        );
        assert_eq!(decode(0x3000_0001), MoodValue::Raw(0x3000_0001));
    }

    #[test]
    fn values_must_fit_the_circuit() {
        assert_eq!(encode(0, MoodValue::On), Ok(1));
        assert_eq!(encode(0, MoodValue::Pct(40)), Ok(40));
        assert_eq!(encode(6, MoodValue::Off), Ok(0x6000_0000));
        assert_eq!(
            encode(5, MoodValue::Temp(24, 2700)),
            Ok(0x5000_0000 + 200_242_700)
        );
        let err = encode(0, MoodValue::Rgb(1, 2, 3)).unwrap_err();
        assert!(err.contains("switch or dimmer"), "{err}");
        assert!(encode(4, MoodValue::Temp(50, 3000)).is_err());
        assert!(encode(6, MoodValue::On).is_err());
        assert!(encode(6, MoodValue::Pct(50)).is_err());
    }

    #[test]
    fn render_factors_the_common_value() {
        assert_eq!(render(&[1; 18]), "all on");
        assert_eq!(render(&[0; 18]), "all off");
        let mut qs = vec![0; 18];
        qs[0] = 1_543_179_980;
        qs[2] = 1;
        assert_eq!(render(&qs), "AQ1 temp(100%, 2700K), AQ3 on");
        let mut qs = vec![1; 18];
        qs[1] = 0;
        assert_eq!(render(&qs), "all on, AQ2 off");
    }
}
