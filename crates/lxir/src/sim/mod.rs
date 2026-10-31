//! The logic simulator behind `lxir test` (D68): runs a compiled
//! `.Loxone` document cycle by cycle, without a Miniserver.
//!
//! Every block with a model ([`model`]) is evaluated once per tick in
//! wiring order; periphery and virtual inputs are sources that read 0
//! until a test sets them. A test may set any port — an input, a
//! parameter, a block's output — and the value holds until changed.
//! Reading a port first checks the logic it depends on: a block without
//! a model, an output its model does not compute, or a fan-in Loxone does
//! not document is an error naming the object, never a guess.

pub mod clock;
pub mod formula;
pub mod model;

use crate::doc::{LoxoneDoc, PortView, ports};
use clock::Clock;
use model::{FanIn, Input, Io, Model, Support};
use std::collections::{BTreeSet, HashMap, HashSet};

struct Object {
    uuid: String,
    block_type: String,
    title: Option<String>,
    support: Support,
    ports: HashMap<String, PortView>,
}

impl Object {
    fn describe(&self) -> String {
        match &self.title {
            Some(t) => format!("{} \"{t}\" ({})", self.block_type, self.uuid),
            None => format!("{} {}", self.block_type, self.uuid),
        }
    }

    fn model(&self) -> Option<&Model> {
        match &self.support {
            Support::Model(m) => Some(m),
            _ => None,
        }
    }
}

pub struct Simulator {
    objects: Vec<Object>,
    by_uuid: HashMap<String, usize>,
    /// Port UUID → (object index, port key).
    port_owner: HashMap<String, (usize, String)>,
    /// Last computed value per port UUID.
    values: HashMap<String, f64>,
    /// Values a test set, per port UUID.
    overrides: HashMap<String, f64>,
    /// Blocks whose model failed (undocumented input, a formula without
    /// a value): their outputs are undefined from then on, an error only
    /// for a test that depends on them.
    faults: HashMap<usize, String>,
    /// Modelled objects in evaluation order: operating modes first, then
    /// wiring order; blocks in a feedback loop follow in document order
    /// and read the previous cycle's values.
    order: Vec<usize>,
    modes: Vec<usize>,
    clock: Clock,
    /// Simulated seconds since the start.
    now: f64,
}

impl Simulator {
    pub fn new(doc: &LoxoneDoc) -> Simulator {
        let mut objects = Vec::new();
        let mut by_uuid = HashMap::new();
        let mut port_owner = HashMap::new();
        for obj in doc.objects() {
            let el = doc.element_at(&obj.path).expect("path from objects()");
            let ports: HashMap<String, PortView> =
                ports(el).into_iter().map(|p| (p.key.clone(), p)).collect();
            let wired = |k: &str| ports.get(k).is_some_and(|p| !p.inputs.is_empty());
            let support = Model::for_object(&obj.block_type, el, &wired);
            let ix = objects.len();
            for p in ports.values() {
                port_owner.insert(p.uuid.clone(), (ix, p.key.clone()));
            }
            by_uuid.insert(obj.uuid.clone(), ix);
            objects.push(Object {
                uuid: obj.uuid,
                block_type: obj.block_type,
                title: obj.title,
                support,
                ports,
            });
        }
        let mut sim = Simulator {
            objects,
            by_uuid,
            port_owner,
            values: HashMap::new(),
            overrides: HashMap::new(),
            faults: HashMap::new(),
            order: Vec::new(),
            modes: Vec::new(),
            clock: Clock::default(),
            now: 0.0,
        };
        sim.order();
        sim
    }

    fn order(&mut self) {
        let modelled: Vec<usize> = (0..self.objects.len())
            .filter(|&i| self.objects[i].model().is_some())
            .collect();
        self.modes = modelled
            .iter()
            .copied()
            .filter(|&i| matches!(self.objects[i].model(), Some(Model::Mode { .. })))
            .collect();
        let mut deps: HashMap<usize, BTreeSet<usize>> = HashMap::new();
        let mut users: HashMap<usize, Vec<usize>> = HashMap::new();
        for &i in &modelled {
            let obj = &self.objects[i];
            let m = obj.model().expect("modelled");
            let mut d = BTreeSet::new();
            for key in m.reads() {
                for src in obj.ports.get(*key).map_or(&[][..], |p| &p.inputs) {
                    if let Some((j, _)) = self.port_owner.get(src)
                        && *j != i
                        && self.objects[*j].model().is_some()
                    {
                        d.insert(*j);
                    }
                }
            }
            for mode in m.mode_deps() {
                if let Some(&j) = self.by_uuid.get(mode) {
                    d.insert(j);
                }
            }
            for &j in &d {
                users.entry(j).or_default().push(i);
            }
            deps.insert(i, d);
        }
        let mut ready: BTreeSet<usize> = modelled
            .iter()
            .copied()
            .filter(|i| deps[i].is_empty())
            .collect();
        let mut done = HashSet::new();
        let mut order = self.modes.clone();
        done.extend(self.modes.iter().copied());
        while let Some(i) = ready.pop_first() {
            if done.insert(i) {
                order.push(i);
            }
            for &u in users.get(&i).map_or(&[][..], |v| v.as_slice()) {
                let d = deps.get_mut(&u).expect("modelled");
                d.remove(&i);
                if d.is_empty() && !done.contains(&u) {
                    ready.insert(u);
                }
            }
        }
        order.extend(modelled.into_iter().filter(|i| !done.contains(i)));
        self.order = order;
    }

    /// The UUID of an object's port.
    pub fn port(&self, object: &str, key: &str) -> Result<String, String> {
        let &ix = self
            .by_uuid
            .get(object)
            .ok_or_else(|| format!("object {object} is not in the compiled config"))?;
        let obj = &self.objects[ix];
        match obj.ports.get(key) {
            Some(p) => Ok(p.uuid.clone()),
            None => {
                let mut keys: Vec<&str> = obj.ports.keys().map(String::as_str).collect();
                keys.sort_unstable();
                Err(format!(
                    "{} has no port `{key}` (it has {})",
                    obj.describe(),
                    keys.join(", ")
                ))
            }
        }
    }

    /// Set a port; the value holds until set again.
    pub fn set(&mut self, port: &str, v: f64) {
        self.overrides.insert(port.to_string(), v);
    }

    pub fn set_clock(&mut self, spec: &str) -> Result<(), String> {
        self.clock.set(spec)
    }

    /// Advance the clock by `dt` seconds and run one program cycle. A
    /// block that fails stops; [`Simulator::check`] reports it.
    pub fn tick(&mut self, dt: f64) {
        self.now += dt;
        self.clock.secs += dt;
        let mut active = HashMap::new();
        for pos in 0..self.order.len() {
            let ix = self.order[pos];
            if pos == self.modes.len() {
                active = self.active_modes();
            }
            if !self.faults.contains_key(&ix)
                && let Err(e) = self.step(ix, &active)
            {
                let at = self.clock_text();
                self.faults.insert(ix, format!("at {at}: {e}"));
            }
        }
    }

    fn clock_text(&self) -> String {
        let secs = self.clock.secs - self.clock.day() as f64 * 86_400.0;
        let secs = secs as u64;
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        format!("t={}s, {h:02}:{m:02}:{s:02}", trim(self.now))
    }

    fn active_modes(&self) -> HashMap<String, bool> {
        self.modes
            .iter()
            .map(|&i| {
                let obj = &self.objects[i];
                let on = obj
                    .ports
                    .get("Q")
                    .is_some_and(|q| self.port_value(&q.uuid) != 0.0);
                (obj.uuid.clone(), on)
            })
            .collect()
    }

    fn step(&mut self, ix: usize, modes: &HashMap<String, bool>) -> Result<(), String> {
        let obj = &self.objects[ix];
        let m = obj.model().expect("ordered objects are modelled");
        let ins: Vec<Input> = m.reads().iter().map(|k| self.input(ix, k)).collect();
        let mut io = Io {
            ins: &ins,
            now: self.now,
            clock: &self.clock,
            modes,
            out: Vec::new(),
        };
        let Support::Model(m) = &mut self.objects[ix].support else {
            unreachable!("checked above")
        };
        m.step(&mut io)?;
        let out = io.out;
        let obj = &self.objects[ix];
        for (key, v) in out {
            if let Some(p) = obj.ports.get(key) {
                self.values.insert(p.uuid.clone(), v);
            }
        }
        Ok(())
    }

    /// A port's own value: what a test set, else what was computed.
    fn port_value(&self, uuid: &str) -> f64 {
        self.overrides
            .get(uuid)
            .or_else(|| self.values.get(uuid))
            .copied()
            .unwrap_or(0.0)
    }

    /// The value an input receives (format facts: loxone-format.md,
    /// *Connectors*): what a test set; else its wires, combined by the
    /// block's fan-in and negated by `Inv=`; else `Inv=` unwired is the
    /// constant 1; else the `Def=` value, or the documented default when
    /// `Def=` is omitted.
    fn input(&self, ix: usize, key: &str) -> Input {
        let obj = &self.objects[ix];
        let open = |v| Input {
            v,
            connected: false,
        };
        let Some(port) = obj.ports.get(key) else {
            return open(documented_default(&obj.block_type, key));
        };
        if let Some(&v) = self.overrides.get(&port.uuid) {
            return Input { v, connected: true };
        }
        if !port.inputs.is_empty() {
            let on = |s: &String| self.port_value(s) != 0.0;
            let fan_in = obj.model().map_or(FanIn::Single, Model::fan_in);
            let mut v = match (port.inputs.len(), fan_in) {
                (1, _) | (_, FanIn::Single) => self.port_value(&port.inputs[0]),
                (_, FanIn::And) => f64::from(u8::from(port.inputs.iter().all(on))),
                (_, FanIn::Or) => f64::from(u8::from(port.inputs.iter().any(on))),
            };
            if port.inv {
                v = if v == 0.0 { 1.0 } else { 0.0 };
            }
            return Input { v, connected: true };
        }
        if port.inv {
            return Input {
                v: 1.0,
                connected: true,
            };
        }
        match port.def.as_deref().and_then(model::parse_num) {
            Some(v) => open(v),
            None => open(documented_default(&obj.block_type, key)),
        }
    }

    /// The current value of a port. Call [`Simulator::check`] first: a
    /// port outside the simulated logic reads 0.
    pub fn read(&self, port: &str) -> f64 {
        if let Some(&v) = self.overrides.get(port) {
            return v;
        }
        let Some((ix, key)) = self.port_owner.get(port) else {
            return 0.0;
        };
        let obj = &self.objects[*ix];
        let is_input = !obj.ports[key].inputs.is_empty()
            || obj
                .model()
                .is_some_and(|m| m.reads().contains(&key.as_str()));
        if is_input {
            self.input(*ix, key).v
        } else {
            self.port_value(port)
        }
    }

    /// Whether the logic behind a port is simulated, given what the test
    /// sets right now. The error names the first object in the way.
    pub fn check(&self, port: &str) -> Result<(), String> {
        let mut seen = HashSet::new();
        self.need(port, &mut seen)
    }

    fn need(&self, port: &str, seen: &mut HashSet<String>) -> Result<(), String> {
        if self.overrides.contains_key(port) || !seen.insert(port.to_string()) {
            return Ok(());
        }
        let Some((ix, key)) = self.port_owner.get(port) else {
            return Err(format!(
                "a wire comes from port {port}, which no object in the config has"
            ));
        };
        let obj = &self.objects[*ix];
        if let Some(fault) = self.faults.get(ix) {
            return Err(format!("{} failed {fault}", obj.describe()));
        }
        let m = match &obj.support {
            Support::Source => return Ok(()),
            Support::Unsupported(why) => {
                return Err(format!(
                    "{} is not simulated: {why} — set a port between it and the \
                     expectation in the test",
                    obj.describe()
                ));
            }
            Support::Model(m) => m,
        };
        let p = &obj.ports[key];
        if !p.inputs.is_empty() || m.reads().contains(&key.as_str()) {
            return self.need_input(obj, m, p, seen);
        }
        if !m.writes().contains(&key.as_str()) {
            return Err(format!(
                "{}: output `{key}` is not simulated (the model computes {})",
                obj.describe(),
                m.writes().join(", ")
            ));
        }
        for k in m.reads() {
            if let Some(p) = obj.ports.get(*k) {
                self.need(&p.uuid, seen)?;
            }
        }
        for mode in m.mode_deps() {
            if let Some(q) = self
                .by_uuid
                .get(mode)
                .and_then(|&j| self.objects[j].ports.get("Q"))
            {
                self.need(&q.uuid, seen)?;
            }
        }
        Ok(())
    }

    fn need_input(
        &self,
        obj: &Object,
        m: &Model,
        p: &PortView,
        seen: &mut HashSet<String>,
    ) -> Result<(), String> {
        if self.overrides.contains_key(&p.uuid) {
            return Ok(());
        }
        if p.inputs.len() > 1 && m.fan_in() == FanIn::Single {
            return Err(format!(
                "{}: {} wires meet at `{}`; how this block combines them is not \
                 documented",
                obj.describe(),
                p.inputs.len(),
                p.key
            ));
        }
        for src in &p.inputs {
            self.need(src, seen)?;
        }
        Ok(())
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The documented default of a parameter (port_meta), else 0 — an open
/// logic input.
fn documented_default(block_type: &str, key: &str) -> f64 {
    crate::port_meta::port_meta(block_type, key)
        .and_then(|m| m.default)
        .and_then(model::parse_num)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests;
