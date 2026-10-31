use super::*;

/// A document with one page holding `objects` (`<C>` lines).
fn doc(objects: &str) -> LoxoneDoc {
    let xml = format!(
        "<ControlList Version=\"1\" NextObj=\"9\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
         <C Type=\"Document\" V=\"175\" U=\"00000001-0000-0000-ffff000000000001\" Title=\"T\">\r\n\
         <C Type=\"Page\" V=\"175\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"P\">\r\n\
         {objects}\
         </C>\r\n\
         </C>\r\n\
         </ControlList>\r\n"
    );
    LoxoneDoc::parse(xml.as_bytes()).unwrap()
}

/// `<C>` of `ty` with uuid `o<n>` and ports `p<n>-<key>`; a port spec is
/// `key`, `key=def`, `key<src` (wired from port uuid `src`), `key!`
/// (Inv) or combinations like `key!<src`.
fn obj(n: u32, ty: &str, ports: &[&str]) -> String {
    obj_attrs(n, ty, "", ports)
}

fn obj_attrs(n: u32, ty: &str, attrs: &str, ports: &[&str]) -> String {
    let mut s = format!("<C Type=\"{ty}\" V=\"175\" U=\"o{n}\" Title=\"{ty}{n}\"{attrs}>\r\n");
    for spec in ports {
        let (head, srcs) = match spec.split_once('<') {
            Some((h, s)) => (h, s.split(',').collect::<Vec<_>>()),
            None => (*spec, Vec::new()),
        };
        let (head, inv) = match head.strip_suffix('!') {
            Some(h) => (h, " Inv=\"true\""),
            None => (head, ""),
        };
        let (key, def) = match head.split_once('=') {
            Some((k, d)) => (k, format!(" Def=\"{d}\"")),
            None => (head, String::new()),
        };
        if srcs.is_empty() {
            s += &format!("<Co K=\"{key}\" U=\"p{n}-{key}\"{def}{inv}/>\r\n");
        } else {
            s += &format!("<Co K=\"{key}\" U=\"p{n}-{key}\"{def}{inv}>\r\n");
            for src in srcs {
                s += &format!("<In Input=\"{src}\"/>\r\n");
            }
            s += "</Co>\r\n";
        }
    }
    s + "</C>\r\n"
}

fn vin(n: u32) -> String {
    obj(n, "VirtualIn", &["AQ"])
}

/// Run ticks of `dt` and return the port's value after each.
fn trace(sim: &mut Simulator, port: &str, dt: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|_| {
            sim.tick(dt);
            sim.read(port)
        })
        .collect()
}

#[test]
fn and_counts_connected_inputs_only() {
    let d = doc(&(vin(1) + &vin(2) + &obj(3, "And", &["I1<p1-AQ", "I2", "I3", "Q"])));
    let mut sim = Simulator::new(&d);
    sim.check("p3-Q").unwrap();
    sim.tick(1.0);
    assert_eq!(sim.read("p3-Q"), 0.0);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    // I2 and I3 are open: only I1 counts.
    assert_eq!(sim.read("p3-Q"), 1.0);
}

#[test]
fn fan_in_is_and_for_and_or_for_or_and_refused_elsewhere() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &obj(3, "And", &["I1<p1-AQ,p2-AQ", "Q"])
        + &obj(4, "Or", &["I1<p1-AQ,p2-AQ", "Q"])
        + &obj(5, "Not", &["I<p1-AQ,p2-AQ", "Q"])));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p3-Q"), 0.0);
    assert_eq!(sim.read("p4-Q"), 1.0);
    let err = sim.check("p5-Q").unwrap_err();
    assert!(err.contains("2 wires meet at `I`"), "{err}");
}

#[test]
fn inv_negates_a_wire_and_is_one_unwired() {
    let d = doc(&(vin(1) + &obj(2, "And", &["I1!<p1-AQ", "I2!", "Q"])));
    let mut sim = Simulator::new(&d);
    sim.tick(1.0);
    assert_eq!(sim.read("p2-Q"), 1.0);
    sim.set("p1-AQ", 5.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p2-Q"), 0.0);
}

#[test]
fn evaluation_follows_wiring_not_document_order() {
    // The Not comes first in the document but reads the Less.
    let d = doc(&(obj(3, "Not", &["I<p2-Q", "Q"])
        + &obj(2, "Less", &["Input1<p1-AQ", "Input2=10", "Q"])
        + &vin(1)));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 3.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p2-Q"), 1.0);
    assert_eq!(sim.read("p3-Q"), 0.0);
}

#[test]
fn arithmetic_and_formula() {
    let d = doc(&(vin(1)
        + &obj(2, "Mult", &["Input1<p1-AQ", "Input2=2,5", "AQ"])
        + &obj_attrs(
            3,
            "Formula",
            " Formula=\"I1*2+I2\"",
            &["Input1<p2-AQ", "Input2=1", "Input3", "Input4", "AQ"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 4.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p2-AQ"), 10.0);
    assert_eq!(sim.read("p3-AQ"), 21.0);
}

#[test]
fn monoflop_pulses_from_the_rising_edge_and_retriggers() {
    let d = doc(&(vin(1)
        + &obj(
            2,
            "Monoflop",
            &["InputTrigger<p1-AQ", "Reset", "Time=3", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    sim.set("p1-AQ", 0.0);
    // Edge at t=1: on until t=4, independent of the input.
    assert_eq!(sim.read("p2-Q"), 1.0);
    assert_eq!(trace(&mut sim, "p2-Q", 1.0, 3), [1.0, 1.0, 0.0]);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0); // t=5, edge
    sim.set("p1-AQ", 0.0);
    sim.tick(1.0);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0); // t=7, retrigger: on until t=10
    assert_eq!(trace(&mut sim, "p2-Q", 1.0, 3), [1.0, 1.0, 0.0]);
}

#[test]
fn monoflop_reset_ends_and_locks() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &obj(
            3,
            "Monoflop",
            &["InputTrigger<p1-AQ", "Reset<p2-AQ", "Time=10", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p3-Q"), 1.0);
    sim.set("p2-AQ", 1.0);
    sim.set("p1-AQ", 0.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p3-Q"), 0.0);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p3-Q"), 0.0, "locked while Reset is on");
}

#[test]
fn on_delay_and_off_delay() {
    let d = doc(&(vin(1)
        + &obj(
            2,
            "OnDelay",
            &["InputTrigger<p1-AQ", "Reset", "Time=2", "Q"],
        )
        + &obj(
            3,
            "OffDelay",
            &["InputTrigger<p1-AQ", "Reset", "Time=2", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0); // input on from t=1
    assert_eq!(sim.read("p2-Q"), 0.0);
    assert_eq!(sim.read("p3-Q"), 1.0);
    assert_eq!(trace(&mut sim, "p2-Q", 1.0, 2), [0.0, 1.0]);
    sim.set("p1-AQ", 0.0);
    sim.tick(1.0); // off at t=4
    assert_eq!(sim.read("p2-Q"), 0.0);
    assert_eq!(sim.read("p3-Q"), 1.0);
    assert_eq!(trace(&mut sim, "p3-Q", 1.0, 2), [1.0, 0.0]);
}

#[test]
fn on_off_delay() {
    let d = doc(&(vin(1)
        + &obj(
            2,
            "OnOffDelay",
            &["InputTrigger<p1-AQ", "Reset", "TimeOn=2", "TimeOff=1", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    assert_eq!(trace(&mut sim, "p2-Q", 1.0, 3), [0.0, 0.0, 1.0]);
    sim.set("p1-AQ", 0.0);
    assert_eq!(trace(&mut sim, "p2-Q", 1.0, 2), [1.0, 0.0]);
}

#[test]
fn switch_toggles_and_on_off_dominate() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &vin(3)
        + &obj(
            4,
            "PushButton",
            &[
                "InputTrigger<p1-AQ",
                "On<p2-AQ",
                "Reset<p3-AQ",
                "InputDisable",
                "Q",
                "Qon",
                "Qoff",
            ],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!((sim.read("p4-Q"), sim.read("p4-Qon")), (1.0, 1.0));
    sim.set("p1-AQ", 0.0);
    sim.tick(1.0);
    assert_eq!((sim.read("p4-Q"), sim.read("p4-Qon")), (1.0, 0.0));
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!((sim.read("p4-Q"), sim.read("p4-Qoff")), (0.0, 1.0));
    sim.set("p2-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p4-Q"), 1.0);
    sim.set("p3-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p4-Q"), 0.0, "Off wins over On");
}

#[test]
fn push_button_holds_for_its_duration() {
    let d = doc(&(vin(1)
        + &obj(
            2,
            "PButtonT",
            &["InputTrigger<p1-AQ", "Reset", "InputDisable", "Time=1", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    assert_eq!(trace(&mut sim, "p2-Q", 0.5, 3), [1.0, 1.0, 0.0]);
}

#[test]
fn threshold_hysteresis_and_edges() {
    let d = doc(&(vin(1)
        + &obj(
            2,
            "AnalogThresholdTrigger",
            &[
                "Input<p1-AQ",
                "On=20",
                "Off=10",
                "PulseTime=1",
                "Q",
                "RisingEdge",
                "FallingEdge",
            ],
        )));
    let mut sim = Simulator::new(&d);
    for (v, q) in [(15.0, 0.0), (21.0, 1.0), (15.0, 1.0), (9.0, 0.0)] {
        sim.set("p1-AQ", v);
        sim.tick(1.0);
        assert_eq!(sim.read("p2-Q"), q, "input {v}");
    }
    assert_eq!(sim.read("p2-FallingEdge"), 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p2-FallingEdge"), 0.0);
}

#[test]
fn sr_flip_flop_set_dominates() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &obj(
            3,
            "SRFlipFlop",
            &["InputS<p1-AQ", "InputR<p2-AQ", "InputTrigger", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    sim.set("p2-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p3-Q"), 1.0);
    sim.set("p1-AQ", 0.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p3-Q"), 0.0);
}

#[test]
fn input_ref_q_follows_ai_when_i_is_open() {
    let d = doc(&(vin(1)
        + &obj(2, "InputRef", &["AI<p1-AQ", "I", "AQ", "Q"])
        + &obj(
            3,
            "Monoflop",
            &["InputTrigger<p2-Q", "Reset", "Time=5", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.check("p3-Q").unwrap();
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p2-AQ"), 1.0);
    assert_eq!(sim.read("p3-Q"), 1.0);
}

#[test]
fn memory_holds_a_set_value_and_passes_its_input() {
    let d = doc(&(obj(1, "Memory", &["Input", "AQ", "Q"])
        + &vin(2)
        + &obj(3, "Memory", &["Input<p2-AQ", "AQ", "Q"])));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 7.0);
    sim.set("p2-AQ", 3.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p1-AQ"), 7.0);
    assert_eq!((sim.read("p3-AQ"), sim.read("p3-Q")), (3.0, 1.0));
}

#[test]
fn unsupported_blocks_and_outputs_are_named() {
    let d = doc(&(obj(1, "PulseGen", &["Q"])
        + &obj(2, "Not", &["I<p1-Q", "Q"])
        + &obj(
            3,
            "Monoflop",
            &["InputTrigger", "Reset", "Time=2", "Q", "AQ"],
        )));
    let mut sim = Simulator::new(&d);
    let err = sim.check("p2-Q").unwrap_err();
    assert!(
        err.contains("PulseGen \"PulseGen1\" (o1) is not simulated"),
        "{err}"
    );
    // Setting the unsupported block's output makes the test independent of it.
    sim.set("p1-Q", 1.0);
    sim.check("p2-Q").unwrap();
    let err = sim.check("p3-AQ").unwrap_err();
    assert!(err.contains("output `AQ` is not simulated"), "{err}");
}

const MODE_MON: &str = "00000000-0000-0004-1500000000000000";
const MODE_TUE: &str = "00000000-0000-0005-1500000000000000";
const MODE_HOLIDAY: &str = "00000000-0000-0001-1500000000000000";

fn mode(uuid: &str, n: u32) -> String {
    format!(
        "<C Type=\"Mode\" V=\"178\" U=\"{uuid}\" Title=\"M{n}\">\r\n\
         <Co K=\"I\" U=\"m{n}-I\"/>\r\n<Co K=\"Q\" U=\"m{n}-Q\"/>\r\n</C>\r\n"
    )
}

fn timer(analog: bool) -> String {
    let analog = if analog {
        " Analog=\"true\" DefValue=\"5\""
    } else {
        ""
    };
    format!(
        "<C Type=\"DayTimer\" V=\"175\" U=\"o9\" Title=\"Timer\" Modes=\"{MODE_MON},{MODE_TUE}\"{analog}>\r\n\
         <Co K=\"InputTrigger\" U=\"p9-InputTrigger\"/>\r\n\
         <Co K=\"Reset\" U=\"p9-Reset\"/>\r\n\
         <Co K=\"Manual\" U=\"p9-Manual\"/>\r\n\
         <Co K=\"PulseTime\" U=\"p9-PulseTime\"/>\r\n\
         <Co K=\"AQ\" U=\"p9-AQ\"/>\r\n\
         <Co K=\"Qon\" U=\"p9-Qon\"/>\r\n\
         <Co K=\"Qoff\" U=\"p9-Qoff\"/>\r\n\
         <Entry To=\"360\" V=\"20\"/>\r\n\
         <Entry From=\"1260\" To=\"1440\" Ix=\"1\" V=\"30\"/>\r\n\
         </C>\r\n"
    )
}

#[test]
fn weekday_modes_follow_the_clock() {
    let d = doc(&(mode(MODE_MON, 1) + &mode(MODE_TUE, 2) + &mode(MODE_HOLIDAY, 3)));
    let mut sim = Simulator::new(&d);
    sim.set_clock("2026-07-13 12:00").unwrap(); // a Monday
    sim.tick(1.0);
    assert_eq!(
        (sim.read("m1-Q"), sim.read("m2-Q"), sim.read("m3-Q")),
        (1.0, 0.0, 0.0)
    );
}

#[test]
fn day_timer_follows_the_active_modes_column() {
    let d = doc(&(mode(MODE_MON, 1) + &mode(MODE_TUE, 2) + &timer(false)));
    let mut sim = Simulator::new(&d);
    sim.check("p9-AQ").unwrap();
    sim.set_clock("2026-07-13 05:00").unwrap(); // Monday, column 0
    sim.tick(1.0);
    assert_eq!(sim.read("p9-AQ"), 1.0);
    sim.set_clock("2026-07-13 22:00").unwrap();
    sim.tick(1.0);
    assert_eq!((sim.read("p9-AQ"), sim.read("p9-Qoff")), (0.0, 1.0));
    sim.set_clock("2026-07-14 22:00").unwrap(); // Tuesday, column 1
    sim.tick(1.0);
    assert_eq!((sim.read("p9-AQ"), sim.read("p9-Qon")), (1.0, 1.0));
}

#[test]
fn analog_day_timer_and_two_active_modes() {
    let d = doc(&(mode(MODE_MON, 1) + &mode(MODE_TUE, 2) + &timer(true)));
    let mut sim = Simulator::new(&d);
    sim.set_clock("2026-07-13 05:00").unwrap();
    sim.tick(1.0);
    assert_eq!(sim.read("p9-AQ"), 20.0);
    sim.set_clock("2026-07-13 12:00").unwrap();
    sim.tick(1.0);
    assert_eq!(sim.read("p9-AQ"), 5.0, "the default value between entries");
    sim.set("m2-Q", 1.0);
    sim.tick(1.0);
    let err = sim.check("p9-AQ").unwrap_err();
    assert!(err.contains("failed at t=3s, 12:00:02"), "{err}");
    assert!(err.contains("2 of its operating modes are active"), "{err}");
}

#[test]
fn feedback_loops_read_the_previous_cycle() {
    // A Not feeding itself oscillates.
    let d = doc(&obj(1, "Not", &["I<p1-Q", "Q"]));
    let mut sim = Simulator::new(&d);
    assert_eq!(trace(&mut sim, "p1-Q", 1.0, 3), [1.0, 0.0, 1.0]);
}

#[test]
fn an_unmodelled_blocks_input_reads_its_wires() {
    // An actuator's command input: the block has no model, the value is
    // what its wire carries.
    let d = doc(&(vin(1)
        + &obj(2, "Not", &["I<p1-AQ", "Q"])
        + &obj(3, "VirtualOutCmd", &["I<p2-Q"])
        + &obj(4, "VirtualOutCmd", &["I"])));
    let mut sim = Simulator::new(&d);
    sim.check("p3-I").unwrap();
    sim.check("p4-I").unwrap();
    sim.tick(1.0);
    assert_eq!((sim.read("p3-I"), sim.read("p4-I")), (1.0, 0.0));
}
