//! `lox energy`: meter readings, the energy flow (`lox energy flow`) and the
//! meters ranked by current power (`lox energy --top`).

use anyhow::Result;
use clap::Subcommand;

use crate::commands::RunContext;
use crate::config::Config;
use crate::energy::{self, FlowReport, MeterPower, Role};
use crate::inventory::is_energy_type;
use crate::util::xml_attr;

#[derive(Subcommand)]
pub(crate) enum EnergyCmd {
    /// Energy flow now: production, consumption, grid import/export, battery
    Flow,
}

pub fn cmd_energy(ctx: &RunContext, room: Option<String>, top: Option<usize>) -> Result<()> {
    if let Some(n) = top {
        return cmd_top(ctx, room.as_deref(), n);
    }
    let mut lox = Config::load()?.client()?;
    let controls = lox.list_controls(None, room.as_deref())?;
    let meters: Vec<_> = controls.iter().filter(|c| is_energy_type(&c.typ)).collect();
    if ctx.json {
        let mut arr = Vec::new();
        for c in &meters {
            let xml = lox.get_all(&c.uuid).unwrap_or_default();
            let val = xml_attr(&xml, "value").unwrap_or("?").to_string();
            arr.push(serde_json::json!({
                "name": c.name, "uuid": c.uuid, "type": c.typ,
                "room": c.room, "value": val,
            }));
        }
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if meters.is_empty() {
        println!("No energy meters found.");
    } else {
        if !ctx.no_header {
            println!("{:<36} {:<20} {:<20} VALUE", "NAME", "TYPE", "ROOM");
            println!("{}", "─".repeat(96));
        }
        for c in &meters {
            let xml = lox.get_all(&c.uuid).unwrap_or_default();
            let val = xml_attr(&xml, "value").unwrap_or("?");
            println!(
                "{:<36} {:<20} {:<20} {}",
                c.name,
                c.typ,
                c.room.as_deref().unwrap_or("─"),
                val
            );
        }
        println!("\n{} energy meters", meters.len());
    }
    Ok(())
}

fn cmd_top(ctx: &RunContext, room: Option<&str>, top: usize) -> Result<()> {
    let mut lox = Config::load()?.client()?;
    let (ranked, total) = energy::top_meters(&mut lox, room, top)?;
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&ranked)?);
    } else if ranked.is_empty() {
        println!("No energy meters found.");
    } else {
        print!("{}", format_top(&ranked, total, ctx.no_header));
    }
    Ok(())
}

pub fn cmd_flow(ctx: &RunContext) -> Result<()> {
    let mut lox = Config::load()?.client()?;
    let report = energy::read_flow(&mut lox)?;
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", format_flow(&report, ctx.no_header));
    }
    Ok(())
}

/// A power value with the Miniserver's unit.
fn power(v: f64, unit: Option<&str>) -> String {
    match unit {
        Some(u) => format!("{:.3} {}", v, u),
        None => format!("{:.3}", v),
    }
}

/// A bar of `width` cells for `v` of `max`.
fn bar(v: f64, max: f64, width: usize) -> String {
    let n = if max > 0.0 {
        ((v.abs() / max) * width as f64).round() as usize
    } else {
        0
    };
    let n = n.min(width);
    format!("{}{}", "█".repeat(n), "░".repeat(width - n))
}

const BAR: usize = 16;

pub fn format_flow(r: &FlowReport, no_header: bool) -> String {
    let unit = r.unit.as_deref();
    let max = [r.production, r.consumption, r.grid, r.battery]
        .into_iter()
        .flatten()
        .map(f64::abs)
        .fold(0.0, f64::max);
    let mut out = String::new();
    if let Some(m) = &r.monitor {
        match &m.room {
            Some(room) => out.push_str(&format!("{} ({})\n\n", m.name, room)),
            None => out.push_str(&format!("{}\n\n", m.name)),
        }
    }
    let mut line = |label: &str, v: Option<f64>, note: &str| {
        let (val, b) = match v {
            Some(v) => (power(v.abs(), unit), bar(v, max, BAR)),
            None => ("—".to_string(), " ".repeat(BAR)),
        };
        out.push_str(
            format!("{:<13}{:>12}  {}  {}", label, val, b, note)
                .trim_end()
                .as_ref(),
        );
        out.push('\n');
    };
    line("Production", r.production, "");
    line("Consumption", r.consumption, "");
    line("Grid", r.grid, r.grid_direction.as_deref().unwrap_or(""));
    let has_battery = r.nodes.iter().any(|n| n.role == Role::Storage);
    if has_battery || r.battery.is_some() || r.battery_soc.is_some() {
        let soc = r
            .battery_soc
            .map(|s| format!("{:.0} % ", s))
            .unwrap_or_default();
        let dir = r.battery_direction.as_deref().unwrap_or("");
        line("Battery", r.battery, &format!("{}{}", soc, dir));
    }
    if let Some(s) = r.self_use {
        out.push_str(&format!("{:<13}{:>10.0} %\n", "Self-use", s));
    }
    if !r.nodes.is_empty() {
        out.push('\n');
        if !no_header {
            out.push_str(&format!("{:<32} {:<11} {:>12}\n", "NODE", "ROLE", "POWER"));
            out.push_str(&format!("{}\n", "─".repeat(57)));
        }
        for n in &r.nodes {
            let role = n.role.as_str();
            let p = n
                .power
                .map(|p| power(p, unit))
                .unwrap_or_else(|| "—".into());
            out.push_str(&format!("{:<32} {:<11} {:>12}\n", n.name, role, p));
        }
        out.push_str(&format!("\n{} nodes\n", r.nodes.len()));
    }
    out
}

pub fn format_top(ranked: &[MeterPower], total: usize, no_header: bool) -> String {
    let mut out = String::new();
    if !no_header {
        out.push_str(&format!(
            "{:>3}  {:<36} {:<24} {:>12}\n",
            "#", "NAME", "ROOM", "POWER"
        ));
        out.push_str(&format!("{}\n", "─".repeat(79)));
    }
    for m in ranked {
        out.push_str(&format!(
            "{:>3}  {:<36} {:<24} {:>12}\n",
            m.rank,
            m.name,
            m.room.as_deref().unwrap_or("─"),
            power(m.power, m.unit.as_deref())
        ));
    }
    out.push_str(&format!("\n{} of {} meters\n", ranked.len(), total));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::energy::{ControlInfo, NodePower};

    fn report() -> FlowReport {
        let node = |role, name: &str, power| NodePower {
            role,
            name: name.into(),
            uuid: None,
            power: Some(power),
        };
        FlowReport {
            monitor: Some(ControlInfo {
                name: "Energiefluss".into(),
                uuid: "efm".into(),
                room: Some("Zentral".into()),
            }),
            unit: Some("kW".into()),
            production: Some(3.2),
            consumption: Some(1.6),
            grid: Some(-0.8),
            grid_direction: Some("exporting".into()),
            battery: Some(0.8),
            battery_direction: Some("charging".into()),
            battery_soc: Some(85.0),
            self_use: Some(75.0),
            nodes: vec![
                node(Role::Grid, "Netzbezug", -0.8),
                node(Role::Production, "PV-Strom", 3.2),
                node(Role::Storage, "Speicher", -0.8),
            ],
        }
    }

    #[test]
    fn flow_table() {
        let s = format_flow(&report(), false);
        let expected = "\
Energiefluss (Zentral)

Production       3.200 kW  ████████████████
Consumption      1.600 kW  ████████░░░░░░░░
Grid             0.800 kW  ████░░░░░░░░░░░░  exporting
Battery          0.800 kW  ████░░░░░░░░░░░░  85 % charging
Self-use             75 %

NODE                             ROLE               POWER
─────────────────────────────────────────────────────────
Netzbezug                        grid           -0.800 kW
PV-Strom                         production      3.200 kW
Speicher                         storage        -0.800 kW

3 nodes
";
        assert_eq!(s, expected, "\n{}", s);
    }

    #[test]
    fn flow_table_without_values_or_battery() {
        let r = FlowReport {
            monitor: None,
            unit: None,
            production: None,
            consumption: None,
            grid: Some(0.0),
            grid_direction: Some("idle".into()),
            battery: None,
            battery_direction: None,
            battery_soc: None,
            self_use: None,
            nodes: vec![],
        };
        let s = format_flow(&r, true);
        assert_eq!(
            s,
            "Production              —\nConsumption             —\nGrid                0.000  ░░░░░░░░░░░░░░░░  idle\n"
        );
    }

    #[test]
    fn top_table() {
        let m = |rank, name: &str, power| MeterPower {
            rank,
            name: name.into(),
            uuid: name.into(),
            room: Some("Zentral".into()),
            power,
            unit: Some("kW".into()),
        };
        let s = format_top(&[m(1, "Heizung", 0.611), m(2, "TV", 0.001)], 31, true);
        assert_eq!(
            s,
            format!(
                "  1  {:<36} {:<24} {:>12}\n  2  {:<36} {:<24} {:>12}\n\n2 of 31 meters\n",
                "Heizung", "Zentral", "0.611 kW", "TV", "Zentral", "0.001 kW"
            )
        );
    }
}
