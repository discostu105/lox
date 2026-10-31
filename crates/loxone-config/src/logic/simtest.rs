//! `loxl test`: plan the `test` bodies against the compiled
//! config, run them in the built-in simulator ([`crate::sim`]), and map
//! the outcome back onto the source `expect` statements.
//!
//! A test addresses ports by slug; the plan resolves each one through the
//! lock to its object's UUID, so titles never need to be unique. The
//! optional lox-cli backend (`loxl test --lox`) still speaks
//! title-addressed `lox sim run` scenario JSON ([`lox_specs`]): it maps
//! every object to its title and refuses titles another object shares —
//! unless the holders carry one signal (Input/OutputRefs mirroring the
//! same target, possibly alongside that target itself), where injecting
//! into all of them at once is exactly right. Running the external
//! binary is the CLI's job; this module is pure.

use super::ast::{Item, Module, PortRef, TestCmp, TestItem, Value};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::lock::Lockfile;
use crate::sim::{Simulator, clock::Clock};
use serde_json::{Map, Value as Json, json};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct TestPlan {
    pub runs: Vec<TestRun>,
}

#[derive(Debug)]
pub struct TestRun {
    pub name: String,
    pub steps: Vec<PlannedStep>,
}

/// What one `tick` executes: the assignments and `clock` before it, the
/// ticks, and the `expect`s that follow.
#[derive(Debug)]
pub struct PlannedStep {
    pub sets: Vec<(Signal, f64)>,
    pub clock: Option<String>,
    pub ticks: u64,
    pub dt: f64,
    pub expects: Vec<PlannedExpect>,
}

/// A port a test names, resolved to its object.
#[derive(Debug, Clone)]
pub struct Signal {
    /// As written (`jal.AutoShade`).
    pub port: PortRef,
    /// The compiled object's UUID.
    pub object: String,
}

#[derive(Debug)]
pub struct PlannedExpect {
    /// The statement as written (`expect jal.AutoShade == 1`), for reports.
    pub line: String,
    pub signal: Signal,
    pub cmp: TestCmp,
    pub expected: f64,
}

/// One test's verdict, mapped back onto its `expect` statements.
pub struct TestResult {
    pub name: String,
    pub pass: bool,
    /// One line per failed expectation: statement, actual value, step.
    pub failures: Vec<String>,
    /// The test could not run: a port outside the simulated logic, a
    /// block whose behavior is not documented enough to simulate.
    pub error: Option<String>,
    /// With [`trace_tests`]: the run cycle by cycle — what each step
    /// sets, every value change of the ports the expectations depend
    /// on, and each expectation's outcome. Empty otherwise.
    pub trace: Vec<String>,
}

/// Build the plan for every `test` in the module (post-expansion,
/// post-desugaring), resolving slugs through the lock to the compiled
/// config's objects. `filter` selects tests by substring of their name.
pub fn plan_tests(
    module: &Module,
    lock: &Lockfile,
    compiled: &LoxoneDoc,
    filter: Option<&str>,
) -> Result<TestPlan> {
    let mut uuid_of = BTreeMap::new();
    for (slug, o) in &lock.objects {
        uuid_of.insert(slug.as_str(), o.uuid.as_str());
    }
    for (slug, e) in &lock.externals {
        uuid_of.insert(slug.as_str(), e.uuid.as_str());
    }
    let present: std::collections::BTreeSet<String> =
        compiled.objects().into_iter().map(|o| o.uuid).collect();
    let resolve = |port: &PortRef, test: &str| -> Result<Signal> {
        let compile_err = |msg: String| Error::Compile(format!("test \"{test}\": {msg}"));
        let uuid = uuid_of.get(port.slug.as_str()).ok_or_else(|| {
            compile_err(format!(
                "`{port}` — slug `{}` is not in the lockfile; only managed blocks \
                 and pinned externs are addressable in the simulator",
                port.slug
            ))
        })?;
        if !present.contains(*uuid) {
            return Err(compile_err(format!(
                "`{port}` — object {uuid} is not in the compiled config"
            )));
        }
        Ok(Signal {
            port: port.clone(),
            object: uuid.to_string(),
        })
    };
    let mut runs = Vec::new();
    for item in &module.items {
        let Item::Test(t) = item else { continue };
        if let Some(f) = filter
            && !t.name.contains(f)
        {
            continue;
        }
        runs.push(plan_one(module, t, &resolve)?);
    }
    Ok(TestPlan { runs })
}

fn plan_one(
    module: &Module,
    t: &super::ast::TestDecl,
    resolve: &dyn Fn(&PortRef, &str) -> Result<Signal>,
) -> Result<TestRun> {
    let compile_err = |msg: String| Err(Error::Compile(format!("test \"{}\": {msg}", t.name)));
    let numeric = |v: &Value| -> Result<f64> {
        let lit = module.resolve_value(v)?;
        lit.parse::<f64>().map_err(|_| {
            Error::Compile(format!(
                "test \"{}\": value `{lit}` is not a number",
                t.name
            ))
        })
    };

    // One step per `tick`: assignments and `clock` gather until a tick
    // executes them; the expects that follow assert on its outcome.
    #[derive(Default)]
    struct StepBuild {
        sets: Vec<(Signal, f64)>,
        clock: Option<String>,
        ticks: Option<(u64, f64)>,
        expects: Vec<PlannedExpect>,
    }
    impl StepBuild {
        fn is_empty(&self) -> bool {
            self.sets.is_empty() && self.clock.is_none() && self.ticks.is_none()
        }
    }

    let mut steps: Vec<StepBuild> = Vec::new();
    let mut cur = StepBuild::default();
    for stmt in &t.body {
        match stmt {
            TestItem::Inject(s) => {
                if cur.ticks.is_some() {
                    steps.push(std::mem::take(&mut cur));
                }
                cur.sets
                    .push((resolve(&s.target, &t.name)?, numeric(&s.value)?));
            }
            TestItem::Clock(c) => {
                if cur.ticks.is_some() {
                    steps.push(std::mem::take(&mut cur));
                }
                if let Err(e) = Clock::default().set(&c.spec) {
                    return compile_err(e);
                }
                cur.clock = Some(c.spec.clone());
            }
            TestItem::Tick(tick) => {
                if cur.ticks.is_some() {
                    steps.push(std::mem::take(&mut cur));
                }
                let dt = match &tick.dt {
                    Some(v) => numeric(v)?,
                    None => 1.0,
                };
                if dt <= 0.0 {
                    return compile_err(format!("`tick {} dt {dt}` — dt must be positive", tick.n));
                }
                cur.ticks = Some((tick.n, dt));
            }
            TestItem::Expect(e) => {
                if cur.ticks.is_none() {
                    return compile_err(format!(
                        "`expect {} {} {}` has no preceding `tick` — assertions read \
                         the state after simulated time has advanced",
                        e.port, e.cmp, e.value
                    ));
                }
                cur.expects.push(PlannedExpect {
                    line: format!("expect {} {} {}", e.port, e.cmp, e.value),
                    signal: resolve(&e.port, &t.name)?,
                    cmp: e.cmp,
                    expected: numeric(&e.value)?,
                });
            }
            TestItem::Comment(_) => {}
        }
    }
    if !cur.is_empty() {
        steps.push(cur);
    }

    if !steps.iter().any(|s| !s.expects.is_empty()) {
        return compile_err("no `expect` — the test asserts nothing".into());
    }
    if let Some(dangling) = steps.last()
        && dangling.ticks.is_none()
    {
        return compile_err(
            "the final assignments are never executed — add a `tick` (and an \
             `expect`) after them"
                .into(),
        );
    }
    let steps = steps
        .into_iter()
        .map(|s| {
            let (ticks, dt) = s
                .ticks
                .expect("stepped only at ticks; dangling refused above");
            PlannedStep {
                sets: s.sets,
                clock: s.clock,
                ticks,
                dt,
                expects: s.expects,
            }
        })
        .collect();
    Ok(TestRun {
        name: t.name.clone(),
        steps,
    })
}

/// Run every planned test in the built-in simulator, each from a fresh
/// start.
pub fn run_tests(plan: &TestPlan, compiled: &LoxoneDoc) -> Vec<TestResult> {
    run_all(plan, compiled, None)
}

/// [`run_tests`], recording a trace of each run (`loxl test --trace`).
/// The lock names the managed blocks and externs by slug; any other
/// object goes by its title.
pub fn trace_tests(plan: &TestPlan, compiled: &LoxoneDoc, lock: &Lockfile) -> Vec<TestResult> {
    let mut slugs = BTreeMap::new();
    for (slug, o) in &lock.objects {
        slugs.insert(o.uuid.clone(), slug.clone());
    }
    for (slug, e) in &lock.externals {
        slugs.insert(e.uuid.clone(), slug.clone());
    }
    run_all(plan, compiled, Some(&slugs))
}

fn run_all(
    plan: &TestPlan,
    compiled: &LoxoneDoc,
    slugs: Option<&BTreeMap<String, String>>,
) -> Vec<TestResult> {
    plan.runs
        .iter()
        .map(|run| {
            let mut failures = Vec::new();
            let mut sim = Simulator::new(compiled);
            let mut tracer = slugs.map(|slugs| Tracer::new(run, &sim, slugs));
            let error = run_one(run, &mut sim, &mut failures, tracer.as_mut()).err();
            TestResult {
                name: run.name.clone(),
                pass: failures.is_empty() && error.is_none(),
                failures,
                error,
                trace: tracer.map(Tracer::finish).unwrap_or_default(),
            }
        })
        .collect()
}

fn run_one(
    run: &TestRun,
    sim: &mut Simulator,
    failures: &mut Vec<String>,
    mut tracer: Option<&mut Tracer>,
) -> std::result::Result<(), String> {
    let port_of = |sim: &Simulator, s: &Signal| {
        sim.port(&s.object, &s.port.port)
            .map_err(|e| format!("`{}`: {e}", s.port))
    };
    for (step_no, step) in run.steps.iter().enumerate() {
        let step_err = |e: String| format!("step {}: {e}", step_no + 1);
        for (signal, v) in &step.sets {
            let port = port_of(sim, signal).map_err(step_err)?;
            sim.set(&port, *v);
            if let Some(t) = tracer.as_deref_mut() {
                t.note(format!(
                    "step {}: set {} = {}",
                    step_no + 1,
                    signal.port,
                    trim_float(*v)
                ));
            }
        }
        if let Some(spec) = &step.clock {
            sim.set_clock(spec).map_err(step_err)?;
            if let Some(t) = tracer.as_deref_mut() {
                t.note(format!("step {}: clock {spec}", step_no + 1));
            }
        }
        for _ in 0..step.ticks {
            sim.tick(step.dt);
            if let Some(t) = tracer.as_deref_mut() {
                t.cycle(sim);
            }
        }
        for e in &step.expects {
            let port = port_of(sim, &e.signal).map_err(step_err)?;
            sim.check(&port).map_err(|why| {
                step_err(format!("`{}` cannot be simulated: {why}", e.signal.port))
            })?;
            let actual = sim.read(&port);
            let ok = holds(e.cmp, actual, e.expected);
            if let Some(t) = tracer.as_deref_mut() {
                let verdict = if ok { "ok" } else { "FAILED" };
                t.note(format!(
                    "step {}: {} — actual {} {verdict}",
                    step_no + 1,
                    e.line,
                    trim_float(actual)
                ));
            }
            if !ok {
                failures.push(format!(
                    "step {}: {} — actual {}",
                    step_no + 1,
                    e.line,
                    trim_float(actual)
                ));
            }
        }
    }
    Ok(())
}

/// Records a run for `loxl test --trace`: the watched ports are every
/// port a set or an expectation of the test touches and the logic they
/// are computed from. The first cycle lists them all, later cycles only
/// the ports whose value changed.
struct Tracer {
    /// (port UUID, label) in evaluation order.
    ports: Vec<(String, String)>,
    last: Option<Vec<f64>>,
    lines: Vec<String>,
    /// Cycle lines written; past [`Tracer::MAX_CYCLES`] the trace stops.
    cycles: usize,
    dropped: usize,
}

impl Tracer {
    const MAX_CYCLES: usize = 500;

    fn new(run: &TestRun, sim: &Simulator, slugs: &BTreeMap<String, String>) -> Tracer {
        let mut roots = Vec::new();
        for step in &run.steps {
            let signals = step
                .sets
                .iter()
                .map(|(s, _)| s)
                .chain(step.expects.iter().map(|e| &e.signal));
            for s in signals {
                if let Ok(p) = sim.port(&s.object, &s.port.port) {
                    roots.push(p);
                }
            }
        }
        let ports = sim.upstream(&roots);
        let ports = ports
            .into_iter()
            .map(|p| {
                let label = match sim.port_owner(&p) {
                    Some((uuid, title, ty, key)) => match (slugs.get(uuid), title) {
                        (Some(slug), _) => format!("{slug}.{key}"),
                        (None, Some(t)) => format!("\"{t}\".{key}"),
                        (None, None) => format!("{ty}.{key}"),
                    },
                    None => p.clone(),
                };
                (p, label)
            })
            .collect();
        Tracer {
            ports,
            last: None,
            lines: Vec::new(),
            cycles: 0,
            dropped: 0,
        }
    }

    fn note(&mut self, line: String) {
        self.flush_dropped();
        self.lines.push(line);
    }

    fn cycle(&mut self, sim: &Simulator) {
        let now: Vec<f64> = self.ports.iter().map(|(p, _)| sim.read(p)).collect();
        let changes: Vec<String> = match &self.last {
            None => self
                .ports
                .iter()
                .zip(&now)
                .map(|((_, label), v)| format!("{label} = {}", trim_float(*v)))
                .collect(),
            Some(last) => self
                .ports
                .iter()
                .zip(now.iter().zip(last))
                .filter(|(_, (v, was))| v != was)
                .map(|((_, label), (v, was))| {
                    format!("{label} {} → {}", trim_float(*was), trim_float(*v))
                })
                .collect(),
        };
        self.last = Some(now);
        if changes.is_empty() {
            return;
        }
        if self.cycles >= Self::MAX_CYCLES {
            self.dropped += 1;
            return;
        }
        self.cycles += 1;
        self.lines
            .push(format!("{}  {}", sim.clock_text(), changes.join(", ")));
    }

    fn finish(mut self) -> Vec<String> {
        self.flush_dropped();
        self.lines
    }

    fn flush_dropped(&mut self) {
        if self.dropped > 0 {
            self.lines.push(format!(
                "… {} more cycles with changes (the trace keeps {})",
                self.dropped,
                Self::MAX_CYCLES
            ));
            self.dropped = 0;
        }
    }
}

/// `==` with the slack of decimal arithmetic, `!=` its negation; `~=`
/// within 5 % of the expected value.
fn holds(cmp: TestCmp, actual: f64, expected: f64) -> bool {
    let close = (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0);
    match cmp {
        TestCmp::Eq => close,
        TestCmp::Ne => !close,
        TestCmp::Ge => actual >= expected || close,
        TestCmp::Gt => actual > expected && !close,
        TestCmp::Le => actual <= expected || close,
        TestCmp::Lt => actual < expected && !close,
        TestCmp::Approx => close || (actual - expected).abs() <= 0.05 * expected.abs(),
    }
}

/// The plan as lox-cli `lox sim run` scenarios (`loxl test --lox`), with
/// the title-addressed key of every `expect` for [`read_results`].
pub struct LoxSpecs {
    /// The `--sim-file` payload: one JSON array of scenario specs.
    pub json: String,
    /// Per run, per step, per expect: the simulator signal key.
    keys: Vec<Vec<Vec<String>>>,
}

/// Translate the plan for lox-cli, which addresses signals as
/// `"<Title>.<Port>"` (or `"<Title> [<Room>].<Port>"`).
pub fn lox_specs(plan: &TestPlan, compiled: &LoxoneDoc) -> Result<LoxSpecs> {
    let titles = TitleTable::build(compiled);
    let mut specs = Vec::new();
    let mut keys = Vec::new();
    for run in &plan.runs {
        let mut json_steps = Vec::new();
        let mut run_keys = Vec::new();
        for step in &run.steps {
            let mut inputs = Map::new();
            for (signal, v) in &step.sets {
                inputs.insert(titles.key(signal, &run.name)?, json!(v));
            }
            let mut expected: Map<String, Json> = Map::new();
            let mut step_keys = Vec::new();
            for e in &step.expects {
                if e.cmp == TestCmp::Ne {
                    return Err(Error::Compile(format!(
                        "test \"{}\": `{}` — lox-cli's `lox sim` has no `!=`; run it in \
                         the built-in simulator (`loxl test` without `--lox`)",
                        run.name, e.line
                    )));
                }
                let key = titles.key(&e.signal, &run.name)?;
                let per_key = expected
                    .entry(key.clone())
                    .or_insert_with(|| Json::Object(Map::new()))
                    .as_object_mut()
                    .expect("built as object");
                if per_key.contains_key(e.cmp.symbol()) {
                    return Err(Error::Compile(format!(
                        "test \"{}\": two `expect {} {} …` in the same step — lox-cli \
                         keeps one comparator per port and step; split them with a \
                         `tick`",
                        run.name,
                        e.signal.port,
                        e.cmp.symbol()
                    )));
                }
                per_key.insert(e.cmp.symbol().to_string(), json!(e.expected));
                step_keys.push(key);
            }
            let mut s = Map::new();
            s.insert("inputs".into(), Json::Object(inputs));
            s.insert("ticks".into(), json!(step.ticks));
            s.insert("dt".into(), json!(step.dt));
            if let Some(clock) = &step.clock {
                s.insert("clock".into(), clock_json(clock));
            }
            s.insert("expected_outputs".into(), Json::Object(expected));
            json_steps.push(Json::Object(s));
            run_keys.push(step_keys);
        }
        // Everything lives in steps; the spec's own phase runs zero ticks.
        specs.push(json!({
            "name": run.name,
            "ticks": 0,
            "steps": json_steps,
        }));
        keys.push(run_keys);
    }
    Ok(LoxSpecs {
        json: serde_json::to_string_pretty(&Json::Array(specs)).expect("specs serialize"),
        keys,
    })
}

/// Object → title resolution for lox-cli.
struct TitleTable {
    /// uuid → (title, block type, `Ref=` target).
    info_of: BTreeMap<String, (Option<String>, String, Option<String>)>,
    /// uuid → the enclosing page's title (the simulator's `[Room]`
    /// qualifier tracks the XML tree's Place/Page nesting).
    room_of: BTreeMap<String, String>,
    /// title → uuids of every object carrying it.
    by_title: BTreeMap<String, Vec<String>>,
}

impl TitleTable {
    fn build(compiled: &LoxoneDoc) -> TitleTable {
        let objects = compiled.objects();
        let mut info_of = BTreeMap::new();
        let mut room_of = BTreeMap::new();
        let mut by_title: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for obj in objects {
            let el = compiled.element_at(&obj.path).expect("path from objects()");
            let refs = el.attr_decoded("Ref").map(|r| r.into_owned());
            // The simulator's "[Room]" qualifier is the enclosing
            // Place/Page element's title in the XML tree (the page the
            // block sits on) — NOT the assigned `IoData Pr=` room.
            for cut in (0..obj.path.len()).rev() {
                let Some(anc) = compiled.element_at(&obj.path[..cut]) else {
                    continue;
                };
                if anc
                    .attr("Type")
                    .is_some_and(|t| t == "Place" || t == "Page")
                {
                    if let Some(title) = anc.attr_decoded("Title") {
                        room_of.insert(obj.uuid.clone(), title.into_owned());
                    }
                    break;
                }
            }
            if let Some(title) = &obj.title {
                by_title
                    .entry(title.clone())
                    .or_default()
                    .push(obj.uuid.clone());
            }
            info_of.insert(obj.uuid.clone(), (obj.title, obj.block_type, refs));
        }
        TitleTable {
            info_of,
            room_of,
            by_title,
        }
    }

    /// The simulator key for a signal, or a pointed error when the title
    /// cannot address the object unambiguously.
    fn key(&self, signal: &Signal, test_name: &str) -> Result<String> {
        let compile_err = |msg: String| Err(Error::Compile(format!("test \"{test_name}\": {msg}")));
        let (port, uuid) = (&signal.port, &signal.object);
        let Some((title, block_type, _)) = self.info_of.get(uuid) else {
            return compile_err(format!(
                "`{port}` — object {uuid} is not in the compiled config"
            ));
        };
        let Some(title) = title else {
            return compile_err(format!(
                "`{port}` — the {block_type} object has no title; lox-cli \
                 addresses blocks by title"
            ));
        };
        let holders = self.by_title.get(title).map_or(&[][..], |v| v.as_slice());
        if holders.len() > 1 && !self.same_signal_mirrors(holders) {
            // The simulator also registers "Title [Room].Port" — use it
            // when the room separates this object from its namesakes
            // (the composite-extern pattern: two "Freigabe" in
            // different rooms).
            let room = self.room_of.get(uuid);
            let room_separates = room.is_some_and(|room| {
                holders
                    .iter()
                    .filter(|u| *u != uuid)
                    .all(|u| self.room_of.get(u) != Some(room))
            });
            match room {
                Some(room) if room_separates => {
                    return Ok(format!("{title} [{room}].{}", port.port));
                }
                _ => {
                    return compile_err(format!(
                        "`{port}` — title \"{title}\" names {} different objects in \
                         the compiled config, so lox-cli cannot address this one; \
                         give the block a unique title or room, or run the \
                         built-in simulator (no --lox)",
                        holders.len()
                    ));
                }
            }
        }
        Ok(format!("{title}.{}", port.port))
    }

    /// Duplicate titles are harmless when the holders all carry the same
    /// signal: every one an Input/OutputRef mirroring the same object,
    /// or one source block plus refs whose `Ref=` points at exactly that
    /// source. The simulator injecting into all of them is then correct.
    fn same_signal_mirrors(&self, uuids: &[String]) -> bool {
        let mut target: Option<String> = None;
        let mut source: Option<&String> = None;
        for uuid in uuids {
            let Some((_, block_type, refs)) = self.info_of.get(uuid) else {
                return false;
            };
            if !matches!(
                block_type.as_str(),
                "InputRef" | "OutputRef" | "OutputRefLM"
            ) {
                if source.is_some() {
                    return false; // two distinct non-ref sources
                }
                source = Some(uuid);
                continue;
            }
            let Some(r) = refs else { return false };
            match &target {
                None => target = Some(r.clone()),
                Some(t) if t == r => {}
                Some(_) => return false,
            }
        }
        match (source, target) {
            // Refs only, all on one target.
            (None, Some(_)) => true,
            // A source and its own mirrors.
            (Some(src), Some(t)) => *src == t,
            // A single non-ref "duplicate" cannot happen (len > 1), and
            // refs without any Ref= never get here — refuse defensively.
            _ => false,
        }
    }
}

/// `"HH:MM[:SS]"` or `"YYYY-MM-DD HH:MM[:SS]"` (validated while planning)
/// into lox-cli's clock spec.
fn clock_json(spec: &str) -> Json {
    match spec.split_once(' ') {
        Some((date, time)) => json!({ "time": time, "date": date }),
        None => json!({ "time": spec }),
    }
}

/// Read `lox sim run --output json` output back onto the plan. The
/// simulator flattens every step's checks into one array per scenario, in
/// step order; within a step (a JSON map, unordered) a check is
/// identified by (signal key, comparator) — [`lox_specs`] refused
/// duplicates. On failing scenarios the CLI appends a second JSON object
/// (the error summary) to stdout — only the first value is the result.
pub fn read_results(stdout: &str, plan: &TestPlan, specs: &LoxSpecs) -> Result<Vec<TestResult>> {
    let sim_err = |msg: String| Err(Error::Compile(format!("sim result: {msg}")));
    let root: Json = serde_json::Deserializer::from_str(stdout.trim())
        .into_iter::<Json>()
        .next()
        .unwrap_or_else(|| serde_json::from_str(""))
        .map_err(|e| Error::Compile(format!("sim output is not JSON ({e}): {stdout}")))?;
    let scenarios = match root.get("scenarios").and_then(Json::as_array) {
        Some(s) => s,
        None => return sim_err(format!("no `scenarios` array in {root}")),
    };
    let mut results = Vec::new();
    for (run, run_keys) in plan.runs.iter().zip(&specs.keys) {
        let Some(scenario) = scenarios
            .iter()
            .find(|s| s.get("name").and_then(Json::as_str) == Some(run.name.as_str()))
        else {
            return sim_err(format!("scenario \"{}\" missing from the output", run.name));
        };
        let checks = scenario
            .get("checks")
            .and_then(Json::as_array)
            .map_or(&[][..], |v| v.as_slice());
        let total: usize = run.steps.iter().map(|s| s.expects.len()).sum();
        if checks.len() != total {
            return sim_err(format!(
                "scenario \"{}\" returned {} checks, the plan has {total}",
                run.name,
                checks.len()
            ));
        }
        let mut failures = Vec::new();
        let mut offset = 0;
        for (step_no, (step, step_keys)) in run.steps.iter().zip(run_keys).enumerate() {
            let slice = &checks[offset..offset + step.expects.len()];
            offset += step.expects.len();
            for (expect, key) in step.expects.iter().zip(step_keys) {
                let cmp = expect.cmp.symbol();
                let Some(check) = slice.iter().find(|c| {
                    c.get("output").and_then(Json::as_str) == Some(key.as_str())
                        && c.get("comparator").and_then(Json::as_str) == Some(cmp)
                }) else {
                    return sim_err(format!(
                        "scenario \"{}\" step {}: no check for `{key}` {cmp}",
                        run.name,
                        step_no + 1,
                    ));
                };
                if check.get("pass").and_then(Json::as_bool) != Some(true) {
                    let actual = check
                        .get("actual")
                        .and_then(Json::as_f64)
                        .map_or("?".to_string(), trim_float);
                    failures.push(format!(
                        "step {}: {} — actual {actual} ({key})",
                        step_no + 1,
                        expect.line,
                    ));
                }
            }
        }
        results.push(TestResult {
            name: run.name.clone(),
            pass: failures.is_empty(),
            failures,
            error: None,
            trace: Vec::new(),
        });
    }
    Ok(results)
}

fn trim_float(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(objects: &str) -> LoxoneDoc {
        let xml = format!(
            "<ControlList Version=\"1\" NextObj=\"9\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
             \t<C Type=\"Document\" V=\"17010727\" U=\"00000001-0000-0000-ffff000000000001\" Title=\"T\" ConfigVersion=\"17010727\">\r\n\
             \t\t<C Type=\"Page\" V=\"175\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"P1\">\r\n\
             {objects}\
             \t\t</C>\r\n\
             \t</C>\r\n\
             </ControlList>\r\n"
        );
        LoxoneDoc::parse(xml.as_bytes()).unwrap()
    }

    fn lock_with(objects: &[(&str, &str)]) -> Lockfile {
        let mut lock = Lockfile::default();
        for (slug, uuid) in objects {
            lock.objects.insert(
                slug.to_string(),
                crate::lock::LockedObject {
                    uuid: uuid.to_string(),
                    block_type: "And".into(),
                    ports: BTreeMap::new(),
                    layout: None,
                    superseded_layout: None,
                    page_uuid: None,
                    expr_owned: false,
                    states: Vec::new(),
                },
            );
        }
        lock
    }

    #[test]
    fn plan_groups_statements_into_steps() {
        let module = Module::parse(
            "a = And(\"Gate A\")\n\
             \n\
             test \"basic\"\n\
             \ta.I1 = 1\n\
             \ta.I2 = 1\n\
             \ttick 3\n\
             \texpect a.Q == 1\n\
             \ta.I1 = 0\n\
             \ttick 2 dt 0.5\n\
             \texpect a.Q == 0\n\
             end\n",
        )
        .unwrap();
        let doc = doc_with(
            "\t\t\t<C Type=\"And\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Gate A\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        let plan = plan_tests(&module, &lock, &doc, None).unwrap();
        assert_eq!(plan.runs.len(), 1);
        assert_eq!(plan.runs[0].steps.len(), 2);
        let specs = lox_specs(&plan, &doc).unwrap();
        let all: Json = serde_json::from_str(&specs.json).unwrap();
        let spec = &all[0];
        assert_eq!(spec["ticks"], 0);
        assert_eq!(spec["steps"][0]["inputs"]["Gate A.I1"], 1.0);
        assert_eq!(spec["steps"][0]["ticks"], 3);
        assert_eq!(spec["steps"][0]["dt"], 1.0);
        assert_eq!(spec["steps"][0]["expected_outputs"]["Gate A.Q"]["=="], 1.0);
        assert_eq!(spec["steps"][1]["dt"], 0.5);
    }

    #[test]
    fn expect_before_tick_and_missing_expect_are_refused() {
        let module =
            Module::parse("a = And()\n\ntest \"t\"\n\ta.I1 = 1\n\texpect a.Q == 1\nend\n").unwrap();
        let doc = doc_with(
            "\t\t\t<C Type=\"And\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"a\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        let err = plan_tests(&module, &lock, &doc, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no preceding `tick`"), "{err}");

        let module = Module::parse("a = And()\n\ntest \"t\"\n\ta.I1 = 1\n\ttick 1\nend\n").unwrap();
        let err = plan_tests(&module, &lock, &doc, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("asserts nothing"), "{err}");
    }

    #[test]
    fn durations_on_off_and_not_equal_plan_as_numbers() {
        let module = Module::parse(
            "a = And(\"Gate A\")\n\
             \n\
             test \"v2\"\n\
             \ta.I1 = on\n\
             \ttick 2min dt 500ms\n\
             \texpect a.Q != off\n\
             end\n",
        )
        .unwrap();
        let doc = doc_with(
            "\t\t\t<C Type=\"And\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Gate A\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        let plan = plan_tests(&module, &lock, &doc, None).unwrap();
        let step = &plan.runs[0].steps[0];
        assert_eq!((step.ticks, step.dt), (240, 0.5));
        assert_eq!(step.sets[0].1, 1.0);
        assert_eq!(
            (step.expects[0].cmp, step.expects[0].expected),
            (TestCmp::Ne, 0.0)
        );
        assert!(holds(TestCmp::Ne, 1.0, 0.0) && !holds(TestCmp::Ne, 0.1 + 0.2, 0.3));
        // lox-cli's scenarios know no `!=`.
        let err = lox_specs(&plan, &doc).err().unwrap().to_string();
        assert!(err.contains("has no `!=`"), "{err}");
    }

    #[test]
    fn ambiguous_titles_are_refused() {
        let module = Module::parse(
            "a = And(\"Dup\")\n\ntest \"t\"\n\ta.I1 = 1\n\ttick 1\n\texpect a.Q == 1\nend\n",
        )
        .unwrap();
        let doc = doc_with(
            "\t\t\t<C Type=\"And\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Dup\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"Or\" V=\"175\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Dup\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        // The plan addresses by UUID: the shared title is no problem for
        // the built-in simulator.
        let plan = plan_tests(&module, &lock, &doc, None).unwrap();
        // lox-cli addresses by title.
        let err = lox_specs(&plan, &doc).err().unwrap().to_string();
        assert!(err.contains("names 2 different objects"), "{err}");
    }

    #[test]
    fn same_signal_ref_duplicates_are_tolerated() {
        let module = Module::parse(
            "a = And()\n\ntest \"t\"\n\ta.I1 = 1\n\ttick 1\n\texpect a.Q == 1\nend\n",
        )
        .unwrap();
        // Two InputRefs of the same title mirroring the SAME object: the
        // duplicate-mirror pattern real configs are full of.
        let doc = doc_with(
            "\t\t\t<C Type=\"And\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"a\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000009-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000009-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        let plan = plan_tests(&module, &lock, &doc, None).unwrap();
        lox_specs(&plan, &doc).unwrap();
    }

    #[test]
    fn source_plus_own_mirrors_is_tolerated() {
        let module = Module::parse(
            "a = Memory()\n\ntest \"t\"\n\ta.AQ = 1\n\ttick 1\n\texpect a.AQ == 1\nend\n",
        )
        .unwrap();
        // A source block sharing its title with InputRefs whose Ref=
        // points back at it (an AI-fed mirror pattern from a real config): injecting the
        // plain title drives the source and its mirrors — one signal.
        let doc = doc_with(
            "\t\t\t<C Type=\"Memory\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Sig\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000003-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000003-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        let plan = plan_tests(&module, &lock, &doc, None).unwrap();
        lox_specs(&plan, &doc).unwrap();

        // But a ref pointing somewhere ELSE than the co-titled source is
        // a different signal — refused.
        let doc = doc_with(
            "\t\t\t<C Type=\"Memory\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Sig\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000009-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n",
        );
        let plan = plan_tests(
            &module,
            &lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]),
            &doc,
            None,
        )
        .unwrap();
        let err = lox_specs(&plan, &doc).err().unwrap().to_string();
        assert!(err.contains("names 2 different objects"), "{err}");
    }

    #[test]
    fn results_map_back_to_expect_lines() {
        let module = Module::parse(
            "a = And(\"Gate A\")\n\
             \n\
             test \"basic\"\n\
             \ta.I1 = 1\n\
             \ttick 3\n\
             \texpect a.Q == 1\n\
             \ttick 2\n\
             \texpect a.Q == 0\n\
             end\n",
        )
        .unwrap();
        let doc = doc_with(
            "\t\t\t<C Type=\"And\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Gate A\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        let plan = plan_tests(&module, &lock, &doc, None).unwrap();
        let stdout = r#"{"pass":false,"passed":0,"total":1,"scenarios":[
            {"name":"basic","pass":false,"checks":[
                {"output":"Gate A.Q","comparator":"==","expected":1.0,"actual":1.0,"pass":true},
                {"output":"Gate A.Q","comparator":"==","expected":0.0,"actual":1.0,"pass":false}
            ]}]}"#;
        let specs = lox_specs(&plan, &doc).unwrap();
        let results = read_results(stdout, &plan, &specs).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].pass);
        assert_eq!(results[0].failures.len(), 1);
        assert!(
            results[0].failures[0].contains("step 2: expect a.Q == 0 — actual 1"),
            "{}",
            results[0].failures[0]
        );
    }

    #[test]
    fn built_in_run_reports_failures_and_unsimulated_logic() {
        let module = Module::parse(
            "a = And(\"Gate A\")\n\
             \n\
             test \"basic\"\n\
             \ta.I1 = 1\n\
             \ta.I2 = 1\n\
             \ttick 1\n\
             \texpect a.Q == 1\n\
             \ta.I2 = 0\n\
             \ttick 1\n\
             \texpect a.Q == 1\n\
             end\n\
             \n\
             test \"bad port\"\n\
             \ttick 1\n\
             \texpect a.Nope == 1\n\
             end\n",
        )
        .unwrap();
        let doc = doc_with(
            "\t\t\t<C Type=\"And\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Gate A\" Nio=\"3\">\r\n\
             \t\t\t\t<Co K=\"I1\" U=\"00000003-0000-0001-ffff000000000001\"/>\r\n\
             \t\t\t\t<Co K=\"I2\" U=\"00000003-0000-0002-ffff000000000001\"/>\r\n\
             \t\t\t\t<Co K=\"Q\" U=\"00000003-0000-0003-ffff000000000001\"/>\r\n\
             \t\t\t</C>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        let plan = plan_tests(&module, &lock, &doc, None).unwrap();
        let results = run_tests(&plan, &doc);
        assert!(!results[0].pass);
        assert_eq!(results[0].failures, ["step 2: expect a.Q == 1 — actual 0"]);
        let err = results[1].error.as_deref().unwrap();
        assert!(err.contains("has no port `Nope`"), "{err}");
    }

    #[test]
    fn comparators_allow_decimal_slack_and_five_percent() {
        assert!(holds(TestCmp::Eq, 0.1 + 0.2, 0.3));
        assert!(!holds(TestCmp::Lt, 0.1 + 0.2, 0.3));
        assert!(holds(TestCmp::Approx, 20.9, 20.0));
        assert!(!holds(TestCmp::Approx, 21.1, 20.0));
        assert!(!holds(TestCmp::Approx, 0.01, 0.0));
    }
}
