//! `lxir test` planning (D36): translate `test` bodies into `lox sim run`
//! scenario specs against the compiled config, and read the simulator's
//! JSON verdict back onto the source `expect` statements.
//!
//! The simulator (lox-cli) addresses signals as `"<Title>.<Port>"`, so the
//! plan maps every referenced slug to its compiled object's title and
//! refuses titles another object shares — unless the holders carry one
//! signal (Input/OutputRefs mirroring the same target, possibly alongside
//! that target itself), where injecting into all of them at once is
//! exactly right. Running the external binary is the CLI's job; this
//! module is pure translation.

use super::ast::{Item, Module, PortRef, TestCmp, TestItem, Value};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::lock::Lockfile;
use serde_json::{Map, Value as Json, json};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct TestPlan {
    pub runs: Vec<TestRun>,
}

impl TestPlan {
    /// The `--sim-file` payload: one JSON array of scenario specs.
    pub fn specs_json(&self) -> String {
        let arr = Json::Array(self.runs.iter().map(|r| r.spec.clone()).collect());
        serde_json::to_string_pretty(&arr).expect("specs serialize")
    }
}

#[derive(Debug)]
pub struct TestRun {
    pub name: String,
    /// Step shape for attributing the flattened result checks.
    pub steps: Vec<PlannedStep>,
    pub spec: Json,
}

#[derive(Debug)]
pub struct PlannedStep {
    pub expects: Vec<PlannedExpect>,
}

#[derive(Debug)]
pub struct PlannedExpect {
    /// The statement as written (`expect jal.AutoShade == 1`), for reports.
    pub line: String,
    /// The simulator signal key (`"<Title>.<Port>"`).
    pub key: String,
    pub cmp: &'static str,
    pub expected: f64,
}

/// One test's verdict, mapped back onto its `expect` statements.
pub struct TestResult {
    pub name: String,
    pub pass: bool,
    /// One line per failed expectation: statement, actual value, step.
    pub failures: Vec<String>,
}

/// Build the sim plan for every `test` in the module (post-expansion,
/// post-desugaring), resolving slugs through the lock into the compiled
/// config's titles. `filter` selects tests by substring of their name.
pub fn plan_tests(
    module: &Module,
    lock: &Lockfile,
    compiled: &LoxoneDoc,
    filter: Option<&str>,
) -> Result<TestPlan> {
    let titles = TitleTable::build(lock, compiled);
    let mut runs = Vec::new();
    for item in &module.items {
        let Item::Test(t) = item else { continue };
        if let Some(f) = filter
            && !t.name.contains(f)
        {
            continue;
        }
        runs.push(plan_one(module, t, &titles)?);
    }
    Ok(TestPlan { runs })
}

/// Slug → title resolution against the compiled config.
struct TitleTable {
    /// slug → object uuid (managed blocks and externs alike).
    uuid_of: BTreeMap<String, String>,
    /// uuid → (title, block type, `Ref=` target).
    info_of: BTreeMap<String, (Option<String>, String, Option<String>)>,
    /// uuid → the enclosing page's title (the simulator's `[Room]`
    /// qualifier tracks the XML tree's Place/Page nesting).
    room_of: BTreeMap<String, String>,
    /// title → uuids of every object carrying it.
    by_title: BTreeMap<String, Vec<String>>,
}

impl TitleTable {
    fn build(lock: &Lockfile, compiled: &LoxoneDoc) -> TitleTable {
        let mut uuid_of = BTreeMap::new();
        for (slug, o) in &lock.objects {
            uuid_of.insert(slug.clone(), o.uuid.clone());
        }
        for (slug, e) in &lock.externals {
            uuid_of.insert(slug.clone(), e.uuid.clone());
        }
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
            uuid_of,
            info_of,
            room_of,
            by_title,
        }
    }

    /// The simulator key for a port reference, or a pointed error when the
    /// title cannot address the object unambiguously.
    fn key(&self, port: &PortRef, test_name: &str) -> Result<String> {
        let compile_err = |msg: String| Err(Error::Compile(format!("test \"{test_name}\": {msg}")));
        let Some(uuid) = self.uuid_of.get(&port.slug) else {
            return compile_err(format!(
                "`{port}` — slug `{}` is not in the lockfile; only managed blocks \
                 and pinned externs are addressable in the simulator",
                port.slug
            ));
        };
        let Some((title, block_type, _)) = self.info_of.get(uuid) else {
            return compile_err(format!(
                "`{port}` — object {uuid} is not in the compiled config",
                uuid = uuid
            ));
        };
        let Some(title) = title else {
            return compile_err(format!(
                "`{port}` — the {block_type} object has no title; the simulator \
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
                         the compiled config, so the simulator cannot address this \
                         one; give the block a unique title or room",
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
            if !matches!(block_type.as_str(), "InputRef" | "OutputRef") {
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

fn plan_one(module: &Module, t: &super::ast::TestDecl, titles: &TitleTable) -> Result<TestRun> {
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
    struct StepBuild {
        inputs: Map<String, Json>,
        clock: Option<Json>,
        ticks: Option<(u64, f64)>,
        expected: Map<String, Json>,
        expects: Vec<PlannedExpect>,
    }
    impl StepBuild {
        fn new() -> StepBuild {
            StepBuild {
                inputs: Map::new(),
                clock: None,
                ticks: None,
                expected: Map::new(),
                expects: Vec::new(),
            }
        }
        fn is_empty(&self) -> bool {
            self.inputs.is_empty() && self.clock.is_none() && self.ticks.is_none()
        }
    }

    let mut steps: Vec<StepBuild> = Vec::new();
    let mut cur = StepBuild::new();
    for stmt in &t.body {
        match stmt {
            TestItem::Inject(s) => {
                if cur.ticks.is_some() {
                    steps.push(std::mem::replace(&mut cur, StepBuild::new()));
                }
                let key = titles.key(&s.target, &t.name)?;
                cur.inputs.insert(key, json!(numeric(&s.value)?));
            }
            TestItem::Clock(c) => {
                if cur.ticks.is_some() {
                    steps.push(std::mem::replace(&mut cur, StepBuild::new()));
                }
                cur.clock = Some(clock_json(&c.spec, &t.name)?);
            }
            TestItem::Tick(tick) => {
                if cur.ticks.is_some() {
                    steps.push(std::mem::replace(&mut cur, StepBuild::new()));
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
                let key = titles.key(&e.port, &t.name)?;
                let cmp = sim_cmp(e.cmp);
                let expected = numeric(&e.value)?;
                let per_key = cur
                    .expected
                    .entry(key.clone())
                    .or_insert_with(|| Json::Object(Map::new()));
                let per_key = per_key.as_object_mut().expect("built as object");
                if per_key.contains_key(cmp) {
                    return compile_err(format!(
                        "two `expect {} {cmp} …` in the same step — the simulator \
                         keeps one comparator per port and step; split them with a \
                         `tick`",
                        e.port
                    ));
                }
                per_key.insert(cmp.to_string(), json!(expected));
                cur.expects.push(PlannedExpect {
                    line: format!("expect {} {} {}", e.port, e.cmp, e.value),
                    key,
                    cmp,
                    expected,
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

    let mut json_steps = Vec::new();
    let mut planned = Vec::new();
    for s in &steps {
        let (n, dt) = s
            .ticks
            .expect("stepped only at ticks; dangling refused above");
        let mut step = Map::new();
        step.insert("inputs".into(), Json::Object(s.inputs.clone()));
        step.insert("ticks".into(), json!(n));
        step.insert("dt".into(), json!(dt));
        if let Some(clock) = &s.clock {
            step.insert("clock".into(), clock.clone());
        }
        step.insert("expected_outputs".into(), Json::Object(s.expected.clone()));
        json_steps.push(Json::Object(step));
        planned.push(PlannedStep {
            expects: s
                .expects
                .iter()
                .map(|e| PlannedExpect {
                    line: e.line.clone(),
                    key: e.key.clone(),
                    cmp: e.cmp,
                    expected: e.expected,
                })
                .collect(),
        });
    }

    // Everything lives in steps; the spec's own phase runs zero ticks.
    let spec = json!({
        "name": t.name,
        "ticks": 0,
        "steps": json_steps,
    });
    Ok(TestRun {
        name: t.name.clone(),
        steps: planned,
        spec,
    })
}

fn sim_cmp(cmp: TestCmp) -> &'static str {
    match cmp {
        TestCmp::Eq => "==",
        TestCmp::Ge => ">=",
        TestCmp::Gt => ">",
        TestCmp::Le => "<=",
        TestCmp::Lt => "<",
        TestCmp::Approx => "~=",
    }
}

/// `"HH:MM[:SS]"` or `"YYYY-MM-DD HH:MM[:SS]"` into the sim's clock spec.
fn clock_json(spec: &str, test_name: &str) -> Result<Json> {
    let compile_err = |msg: String| Err(Error::Compile(format!("test \"{test_name}\": {msg}")));
    let (date, time) = match spec.split_once(' ') {
        Some((d, t)) => (Some(d), t),
        None => (None, spec),
    };
    if let Some(d) = date
        && d.split('-').count() != 3
    {
        return compile_err(format!(
            "clock \"{spec}\" — expected a `YYYY-MM-DD` date before the time"
        ));
    }
    let time_ok = matches!(time.split(':').count(), 2 | 3)
        && time
            .split(':')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if !time_ok {
        return compile_err(format!(
            "clock \"{spec}\" — expected `HH:MM` or `HH:MM:SS` (optionally after a \
             `YYYY-MM-DD` date)"
        ));
    }
    let mut obj = Map::new();
    obj.insert("time".into(), json!(time));
    if let Some(d) = date {
        obj.insert("date".into(), json!(d));
    }
    Ok(Json::Object(obj))
}

/// Read `lox sim run --output json` output back onto the plan. The
/// simulator flattens every step's checks into one array per scenario, in
/// step order; within a step (a JSON map, unordered) a check is
/// identified by (signal key, comparator) — planning refused duplicates.
/// On failing scenarios the CLI appends a second JSON object (the error
/// summary) to stdout — only the first value is the result.
pub fn read_results(stdout: &str, plan: &TestPlan) -> Result<Vec<TestResult>> {
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
    for run in &plan.runs {
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
        for (step_no, step) in run.steps.iter().enumerate() {
            let slice = &checks[offset..offset + step.expects.len()];
            offset += step.expects.len();
            for expect in &step.expects {
                let Some(check) = slice.iter().find(|c| {
                    c.get("output").and_then(Json::as_str) == Some(expect.key.as_str())
                        && c.get("comparator").and_then(Json::as_str) == Some(expect.cmp)
                }) else {
                    return sim_err(format!(
                        "scenario \"{}\" step {}: no check for `{}` {}",
                        run.name,
                        step_no + 1,
                        expect.key,
                        expect.cmp
                    ));
                };
                if check.get("pass").and_then(Json::as_bool) != Some(true) {
                    let actual = check
                        .get("actual")
                        .and_then(Json::as_f64)
                        .map_or("?".to_string(), trim_float);
                    failures.push(format!(
                        "step {}: {} — actual {actual} ({})",
                        step_no + 1,
                        expect.line,
                        expect.key
                    ));
                }
            }
        }
        results.push(TestResult {
            name: run.name.clone(),
            pass: failures.is_empty(),
            failures,
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
        let run = &plan.runs[0];
        assert_eq!(run.steps.len(), 2);
        let spec = &run.spec;
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
        let err = plan_tests(&module, &lock, &doc, None)
            .unwrap_err()
            .to_string();
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
        plan_tests(&module, &lock, &doc, None).unwrap();
    }

    #[test]
    fn source_plus_own_mirrors_is_tolerated() {
        let module = Module::parse(
            "a = Memory()\n\ntest \"t\"\n\ta.AQ = 1\n\ttick 1\n\texpect a.AQ == 1\nend\n",
        )
        .unwrap();
        // A source block sharing its title with InputRefs whose Ref=
        // points back at it (r50's AI-fed mirror pattern): injecting the
        // plain title drives the source and its mirrors — one signal.
        let doc = doc_with(
            "\t\t\t<C Type=\"Memory\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Sig\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000003-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000003-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n",
        );
        let lock = lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]);
        plan_tests(&module, &lock, &doc, None).unwrap();

        // But a ref pointing somewhere ELSE than the co-titled source is
        // a different signal — refused.
        let doc = doc_with(
            "\t\t\t<C Type=\"Memory\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Sig\" Nio=\"3\"/>\r\n\
             \t\t\t<C Type=\"InputRef\" V=\"175\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Sig\" Ref=\"00000009-0000-0000-ffff000000000001\" Nio=\"3\"/>\r\n",
        );
        let err = plan_tests(
            &module,
            &lock_with(&[("a", "00000003-0000-0000-ffff000000000001")]),
            &doc,
            None,
        )
        .unwrap_err()
        .to_string();
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
        let results = read_results(stdout, &plan).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].pass);
        assert_eq!(results[0].failures.len(), 1);
        assert!(
            results[0].failures[0].contains("step 2: expect a.Q == 0 — actual 1"),
            "{}",
            results[0].failures[0]
        );
    }
}
