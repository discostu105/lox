//! The logic simulator behind `loxl test`: runs a compiled
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
use model::{FanIn, Input, Io, Model, ShadeCmd, Support};
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
    /// Shade → the `CentralShade`s whose `rec=` lists it.
    centrals_of: HashMap<usize, Vec<usize>>,
    /// The command each central block gives this cycle.
    shade_cmds: HashMap<usize, ShadeCmd>,
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
            centrals_of: HashMap::new(),
            shade_cmds: HashMap::new(),
            clock: Clock::default(),
            now: 0.0,
        };
        sim.link_centrals();
        sim.order();
        sim
    }

    /// Find the shades of each `CentralShade`. A central block with a
    /// shade that has no model is not simulated.
    fn link_centrals(&mut self) {
        for c in 0..self.objects.len() {
            let Some(Model::Central(central)) = self.objects[c].model() else {
                continue;
            };
            let mut members = Vec::new();
            let mut why = None;
            for uuid in &central.members {
                match self.by_uuid.get(uuid) {
                    Some(&ix) if matches!(self.objects[ix].model(), Some(Model::Blind(_))) => {
                        members.push(ix)
                    }
                    Some(&ix) => {
                        why = Some(format!(
                            "its shade {} has no model",
                            self.objects[ix].describe()
                        ))
                    }
                    None => why = Some(format!("its shade {uuid} is not in the config")),
                }
            }
            match why {
                Some(why) => self.objects[c].support = Support::Unsupported(why),
                None => {
                    for ix in members {
                        self.centrals_of.entry(ix).or_default().push(c);
                    }
                }
            }
        }
    }

    fn members(&self, ix: usize) -> &[String] {
        match self.objects[ix].model() {
            Some(Model::Central(c)) => &c.members,
            _ => &[],
        }
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
            d.extend(self.centrals_of.get(&i).into_iter().flatten().copied());
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
        self.shade_cmds.clear();
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
        self.count_shades();
    }

    /// No and Nc of every central block: its shades at rest fully open
    /// (position 0) and fully closed (1). [`Simulator::need`] refuses them
    /// while a shade moves or rests in between.
    fn count_shades(&mut self) {
        for c in 0..self.objects.len() {
            if self.faults.contains_key(&c)
                || !matches!(self.objects[c].model(), Some(Model::Central(_)))
            {
                continue;
            }
            let (mut open, mut closed) = (0.0, 0.0);
            for uuid in self.members(c) {
                if let Some((pos, false)) = self.shade_state(uuid) {
                    if pos <= 0.0 {
                        open += 1.0;
                    } else if pos >= 1.0 {
                        closed += 1.0;
                    }
                }
            }
            for (key, n) in [("OutOpen", open), ("OutClose", closed)] {
                if let Some(p) = self.objects[c].ports.get(key) {
                    self.values.insert(p.uuid.clone(), n);
                }
            }
        }
    }

    /// A shade's position and whether it moves.
    fn shade_state(&self, uuid: &str) -> Option<(f64, bool)> {
        match self.objects[*self.by_uuid.get(uuid)?].model() {
            Some(Model::Blind(b)) => Some(b.state()),
            _ => None,
        }
    }

    /// The simulated time for reports: `t=12.5s, 00:00:12`.
    pub fn clock_text(&self) -> String {
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
        let central: Vec<ShadeCmd> = self
            .centrals_of
            .get(&ix)
            .into_iter()
            .flatten()
            .filter_map(|c| self.shade_cmds.get(c).copied())
            .collect();
        let mut io = Io {
            ins: &ins,
            now: self.now,
            clock: &self.clock,
            modes,
            central: &central,
            out: Vec::new(),
        };
        let Support::Model(m) = &mut self.objects[ix].support else {
            unreachable!("checked above")
        };
        m.step(&mut io)?;
        if let Model::Central(c) = m
            && let Some(cmd) = c.cmd
        {
            self.shade_cmds.insert(ix, cmd);
        }
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
    /// constant 1; else the `Def=` value, 0 when `Def=` is omitted; the
    /// GUI default for a port the object has no connector for, which a
    /// save adds at that value.
    fn input(&self, ix: usize, key: &str) -> Input {
        let obj = &self.objects[ix];
        let open = |v| Input {
            v,
            connected: false,
        };
        let Some(port) = obj.ports.get(key) else {
            return open(missing_default(&obj.block_type, key));
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
        // An absent `Def=` is 0: a save writes none at 0.
        open(
            port.def
                .as_deref()
                .and_then(model::parse_num)
                .unwrap_or(0.0),
        )
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
        if self.is_input(*ix, key) {
            self.input(*ix, key).v
        } else {
            self.port_value(port)
        }
    }

    /// An input or parameter: wired (wires are stored at the sink), read
    /// by the block's model, or an input in the verified connector table.
    /// Its value depends on its wires only, so it can be read on any
    /// block, a block without a model too (an actuator's command input).
    fn is_input(&self, ix: usize, key: &str) -> bool {
        let obj = &self.objects[ix];
        !obj.ports[key].inputs.is_empty()
            || obj.model().is_some_and(|m| m.reads().contains(&key))
            || crate::connectors::builtin(&obj.block_type).is_some_and(|ports| {
                ports.iter().any(|p| {
                    p.key == key
                        && matches!(
                            p.dir,
                            crate::connectors::PortDir::Input | crate::connectors::PortDir::Param
                        )
                })
            })
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
        if matches!(obj.support, Support::Source) {
            return Ok(());
        }
        if self.is_input(*ix, key) {
            return self.need_input(obj, &obj.ports[key], seen);
        }
        if let Some(fault) = self.faults.get(ix) {
            return Err(format!("{} failed {fault}", obj.describe()));
        }
        let m = match &obj.support {
            Support::Source => unreachable!("returned above"),
            Support::Unsupported(why) => {
                return Err(format!(
                    "{} is not simulated: {why}. Set a port between it and the expectation in the test.",
                    obj.describe()
                ));
            }
            Support::Model(m) => m,
        };
        if !m.writes().contains(&key.as_str()) {
            return Err(format!(
                "{}: output `{key}` is not simulated (the model computes {})",
                obj.describe(),
                m.writes().join(", ")
            ));
        }
        self.need_block(*ix, seen)?;
        if matches!(m, Model::Central(_)) {
            // No and Nc count the shades fully open and fully closed; what
            // counts as open or closed in between is undocumented.
            for uuid in self.members(*ix) {
                let j = self.by_uuid[uuid];
                if let Some(fault) = self.faults.get(&j) {
                    return Err(format!("{} failed {fault}", self.objects[j].describe()));
                }
                self.need_block(j, seen)?;
                if let Some((pos, moving)) = self.shade_state(uuid)
                    && (moving || (pos > 0.0 && pos < 1.0))
                {
                    return Err(format!(
                        "{}: its shade {} moves or rests between open and closed, and \
                         whether No and Nc count it is undocumented",
                        obj.describe(),
                        self.objects[j].describe()
                    ));
                }
            }
        }
        Ok(())
    }

    /// The logic a modelled block's outputs depend on: the inputs it
    /// reads, the operating modes it follows, the central blocks that
    /// command it.
    fn need_block(&self, ix: usize, seen: &mut HashSet<String>) -> Result<(), String> {
        let obj = &self.objects[ix];
        let m = obj.model().expect("a modelled block");
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
        for &c in self.centrals_of.get(&ix).into_iter().flatten() {
            if let Some(fault) = self.faults.get(&c) {
                return Err(format!("{} failed {fault}", self.objects[c].describe()));
            }
            self.need_block(c, seen)?;
        }
        Ok(())
    }

    /// The ports the given ports' values are computed from, themselves
    /// included, in evaluation order: sources first, then the modelled
    /// blocks, then the blocks without a model. The walk follows wires
    /// into inputs, the inputs a modelled block reads behind its outputs
    /// and the operating modes a schedule follows; sources and blocks
    /// without a model end it. What `loxl test --trace` watches.
    pub fn upstream(&self, roots: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut stack = roots.to_vec();
        while let Some(p) = stack.pop() {
            let Some((ix, key)) = self.port_owner.get(&p) else {
                continue;
            };
            if !seen.insert(p.clone()) {
                continue;
            }
            let obj = &self.objects[*ix];
            if self.is_input(*ix, key) {
                stack.extend(obj.ports[key].inputs.iter().cloned());
            } else if let Some(m) = obj.model() {
                for k in m.reads() {
                    if let Some(p) = obj.ports.get(*k) {
                        stack.push(p.uuid.clone());
                    }
                }
                for mode in m.mode_deps() {
                    if let Some(q) = self
                        .by_uuid
                        .get(mode)
                        .and_then(|&j| self.objects[j].ports.get("Q"))
                    {
                        stack.push(q.uuid.clone());
                    }
                }
                // A shade's central blocks, and a central block's shades.
                let linked = self.centrals_of.get(ix).into_iter().flatten().copied();
                let members = self.members(*ix).iter().map(|u| self.by_uuid[u]);
                for j in linked.chain(members) {
                    let o = &self.objects[j];
                    if let Some(jm) = o.model() {
                        for k in jm.reads() {
                            if let Some(p) = o.ports.get(*k) {
                                stack.push(p.uuid.clone());
                            }
                        }
                    }
                }
            }
        }
        let pos: HashMap<usize, usize> = self
            .order
            .iter()
            .enumerate()
            .map(|(pos, &ix)| (ix, pos + 1))
            .collect();
        let mut ports: Vec<String> = seen.into_iter().collect();
        ports.sort_by_key(|p| {
            let (ix, key) = &self.port_owner[p];
            let rank = match self.objects[*ix].support {
                Support::Source => 0,
                Support::Model(_) => pos[ix],
                Support::Unsupported(_) => usize::MAX,
            };
            let input_first = !self.is_input(*ix, key);
            (rank, *ix, input_first, key.clone())
        });
        ports
    }

    /// The owner of a port: its object's UUID, title and type, and the
    /// port key.
    pub fn port_owner(&self, port: &str) -> Option<(&str, Option<&str>, &str, &str)> {
        let (ix, key) = self.port_owner.get(port)?;
        let obj = &self.objects[*ix];
        Some((&obj.uuid, obj.title.as_deref(), &obj.block_type, key))
    }

    fn need_input(
        &self,
        obj: &Object,
        p: &PortView,
        seen: &mut HashSet<String>,
    ) -> Result<(), String> {
        if self.overrides.contains_key(&p.uuid) {
            return Ok(());
        }
        let fan_in = obj.model().map_or(FanIn::Single, Model::fan_in);
        if p.inputs.len() > 1 && fan_in == FanIn::Single {
            return Err(format!(
                "{}: {} wires meet at `{}`, and how this block combines them is not documented",
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

/// What a missing connector reads: its GUI default, else the documented
/// default (port_meta), else 0 — an open logic input.
fn missing_default(block_type: &str, key: &str) -> f64 {
    crate::gui_defaults::gui_default(block_type, key)
        .or_else(|| crate::port_meta::port_meta(block_type, key).and_then(|m| m.default))
        .and_then(model::parse_num)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests;
