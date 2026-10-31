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

#[test]
fn upstream_walks_wires_and_models_in_evaluation_order() {
    let d = doc(&(obj(3, "VirtualOutCmd", &["I<p2-Q"])
        + &obj(2, "Not", &["I<p1-AQ", "Q"])
        + &vin(1)
        + &vin(4)));
    let sim = Simulator::new(&d);
    // Source, then the model (inputs before outputs), then the sink;
    // the unrelated input is not watched.
    assert_eq!(
        sim.upstream(&["p3-I".to_string()]),
        ["p1-AQ", "p2-I", "p2-Q", "p3-I"]
    );
}

#[test]
fn push_switch_pulses_on_both_edges_and_latches_with_on() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &vin(3)
        + &obj(
            4,
            "Switch2Button",
            &[
                "InputTrigger<p1-AQ",
                "On<p2-AQ",
                "Reset<p3-AQ",
                "InputDisable",
                "Time=0,5",
                "Q",
                "Qon",
                "Qoff",
            ],
        )));
    let mut sim = Simulator::new(&d);
    sim.check("p4-Q").unwrap();
    let read = |sim: &Simulator| (sim.read("p4-Q"), sim.read("p4-Qon"), sim.read("p4-Qoff"));
    sim.set("p1-AQ", 1.0);
    sim.tick(0.1);
    assert_eq!(read(&sim), (1.0, 1.0, 0.0), "rising edge");
    sim.tick(0.5);
    assert_eq!(read(&sim), (0.0, 0.0, 0.0), "Don over while Tr is still on");
    sim.set("p1-AQ", 0.0);
    sim.tick(0.1);
    assert_eq!(read(&sim), (1.0, 0.0, 1.0), "falling edge");
    sim.tick(0.5);
    sim.set("p2-AQ", 1.0);
    sim.tick(0.1);
    sim.set("p2-AQ", 0.0);
    sim.tick(1.0);
    assert_eq!(read(&sim), (1.0, 0.0, 0.0), "On latches O");
    sim.set("p3-AQ", 1.0);
    sim.tick(0.1);
    assert_eq!(read(&sim), (0.0, 0.0, 1.0), "Off switches off");
    sim.tick(1.0);
    sim.set("p1-AQ", 1.0);
    sim.tick(0.1);
    assert_eq!(read(&sim), (0.0, 0.0, 0.0), "Off held locks");
}

#[test]
fn push_switch_refuses_undocumented_edges() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &obj(
            4,
            "Switch2Button",
            &[
                "InputTrigger<p1-AQ",
                "On<p2-AQ",
                "Reset",
                "InputDisable",
                "Time=0,02",
                "Q",
            ],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p2-AQ", 1.0);
    sim.tick(0.1);
    sim.set("p1-AQ", 1.0);
    sim.tick(0.1);
    let err = sim.check("p4-Q").unwrap_err();
    assert!(err.contains("while On holds O on"), "{err}");
}

#[test]
fn pulse_at_fires_when_time_passes_the_configured_time() {
    let d = doc(&(vin(1)
        + &obj_attrs(
            2,
            "PulseAt",
            " Sec=\"14400\" Typ=\"0\"",
            &["InputDisable<p1-AQ", "Time=2", "Q"],
        )));
    let mut sim = Simulator::new(&d);
    sim.check("p2-Q").unwrap();
    sim.set_clock("03:59:58").unwrap();
    assert_eq!(trace(&mut sim, "p2-Q", 1.0, 5), [0.0, 1.0, 1.0, 0.0, 0.0]);
    // A clock statement that jumps over the time starts no pulse.
    sim.set_clock("2026-01-02 03:00").unwrap();
    sim.tick(1.0);
    sim.set_clock("2026-01-02 05:00").unwrap();
    sim.tick(1.0);
    assert_eq!(sim.read("p2-Q"), 0.0);
    // A tick longer than the pulse still shows it for one cycle.
    sim.set_clock("2026-01-03 03:59").unwrap();
    assert_eq!(trace(&mut sim, "p2-Q", 600.0, 2), [1.0, 0.0]);
    // Off ends a running pulse.
    sim.set_clock("2026-01-04 03:59:59").unwrap();
    sim.tick(1.0);
    assert_eq!(sim.read("p2-Q"), 1.0);
    sim.set("p1-AQ", 1.0);
    sim.tick(0.1);
    sim.set("p1-AQ", 0.0);
    sim.tick(0.1);
    assert_eq!(sim.read("p2-Q"), 0.0);
}

#[test]
fn pulse_at_refuses_other_kinds() {
    let d = doc(&obj_attrs(
        2,
        "PulseAt",
        " Sec=\"0\" Typ=\"3\"",
        &["InputDisable", "Time=1", "Q"],
    ));
    let sim = Simulator::new(&d);
    let err = sim.check("p2-Q").unwrap_err();
    assert!(err.contains("Typ=\"3\""), "{err}");
}

#[test]
fn fixed_value_meter_counts_the_rating_while_on() {
    let d = doc(&(vin(1)
        + &obj(
            2,
            "MeterDig",
            &["A<p1-AQ", "R", "Ppwr=2", "Mro=10", "OPf", "OMr"],
        )));
    let mut sim = Simulator::new(&d);
    sim.check("p2-OMr").unwrap();
    sim.tick(1.0);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!((sim.read("p2-OPf"), sim.read("p2-OMr")), (2.0, 10.0));
    sim.tick(1800.0);
    // 2 kW for half an hour: 1 kWh.
    assert_eq!(sim.read("p2-OMr"), 11.0);
    sim.set("p1-AQ", 0.0);
    sim.tick(3600.0);
    sim.tick(3600.0);
    // A step counts with the power of the cycle before it: the hour up to
    // the cycle that sees S = 0 still runs at 2 kW.
    assert_eq!((sim.read("p2-OPf"), sim.read("p2-OMr")), (0.0, 13.0));
    sim.set("p1-AQ", 50.0);
    sim.tick(1.0);
    let err = sim.check("p2-OMr").unwrap_err();
    assert!(err.contains("S = 50"), "{err}");
}

#[test]
fn meter_integrates_pf_alone_and_maps_a_reading() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &obj(
            3,
            "MeterAbsUni",
            &["Pf<p1-AQ", "Mr", "R", "Mro", "OPf", "OMr"],
        )
        + &obj(
            4,
            "MeterAbsUni",
            &["Pf<p1-AQ", "Mr<p2-AQ", "R", "Mro=1", "OPf", "OMr"],
        )));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 3.0);
    sim.set("p2-AQ", 100.0);
    sim.tick(1.0);
    sim.tick(3600.0);
    assert_eq!((sim.read("p3-OPf"), sim.read("p3-OMr")), (3.0, 3.0));
    assert_eq!((sim.read("p4-OPf"), sim.read("p4-OMr")), (3.0, 101.0));
    // A decreasing reading is discarded.
    sim.set("p2-AQ", 90.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p4-OMr"), 101.0);
    sim.set("p2-AQ", 120.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p4-OMr"), 121.0);
}

#[test]
fn meters_refuse_reset_and_undocumented_inputs() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &obj(
            3,
            "MeterAbsUni",
            &["Pf", "Mr<p2-AQ", "R", "Mro", "OPf", "OMr"],
        )
        + &obj(
            4,
            "MeterAbsBi",
            &[
                "Pf<p1-AQ", "Mr1", "Mr2", "R", "Mro1", "Mro2", "OPf", "OMr1", "OMr2",
            ],
        )
        + &obj(
            5,
            "MeterDig",
            &["A", "R<p1-AQ", "Ppwr", "Mro", "OPf", "OMr"],
        )
        + &obj(6, "MeterDig", &["A", "R", "Ppwr", "Mro", "ORd"])));
    let mut sim = Simulator::new(&d);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    let err = sim.check("p3-OMr").unwrap_err();
    assert!(err.contains("Mr without Pf"), "{err}");
    let err = sim.check("p4-OMr1").unwrap_err();
    assert!(err.contains("Pf alone"), "{err}");
    let err = sim.check("p5-OMr").unwrap_err();
    assert!(err.contains("R is on"), "{err}");
    let err = sim.check("p6-ORd").unwrap_err();
    assert!(err.contains("output `ORd` is not simulated"), "{err}");
}

#[test]
fn bidirectional_meter_maps_both_readings() {
    let d = doc(&(vin(1)
        + &vin(2)
        + &vin(3)
        + &obj(
            4,
            "MeterAbsBi",
            &[
                "Pf<p1-AQ",
                "Mr1<p2-AQ",
                "Mr2<p3-AQ",
                "R",
                "Mro1=5",
                "Mro2",
                "OPf",
                "OMr1",
                "OMr2",
            ],
        )));
    let mut sim = Simulator::new(&d);
    sim.check("p4-OMr2").unwrap();
    sim.set("p1-AQ", -2.0);
    sim.set("p2-AQ", 10.0);
    sim.set("p3-AQ", 20.0);
    sim.tick(1.0);
    assert_eq!(
        (sim.read("p4-OPf"), sim.read("p4-OMr1"), sim.read("p4-OMr2")),
        (-2.0, 15.0, 20.0)
    );
}

#[test]
fn overtemp_shutdown_is_a_source() {
    let d = doc(&(obj(1, "OvertempShutdown", &["Q"]) + &obj(2, "Not", &["I<p1-Q", "Q"])));
    let mut sim = Simulator::new(&d);
    sim.check("p2-Q").unwrap();
    sim.tick(1.0);
    assert_eq!(sim.read("p2-Q"), 1.0);
}

/// An `AutoJalousie` `o<n>` (roller blind, Opd 60 s, Cld 50 s) with
/// Co/Cc/So/Wa/Dwc/Off wired from virtual inputs 1…6.
fn blind(n: u32, extra: &[&str]) -> String {
    let mut ports = vec![
        "EndUp<p1-AQ",
        "EndDown<p2-AQ",
        "Shade<p3-AQ",
        "Safety<p4-AQ",
        "Window<p5-AQ",
        "Stop<p6-AQ",
        "TimeEnd=60",
        "TimeEndDown=50",
        "Type=1",
        "Back=0,6",
        "OutputUp",
        "OutputDown",
        "OutputPos",
        "OutputSafety",
        "OutputLock",
        "OutputLPos",
    ];
    ports.extend_from_slice(extra);
    (1..=6).map(vin).collect::<String>() + &obj(n, "AutoJalousie", &ports)
}

fn pulse(sim: &mut Simulator, port: &str) {
    sim.set(port, 1.0);
    sim.tick(0.1);
    sim.set(port, 0.0);
}

#[test]
fn blind_closes_for_the_remaining_run_time_and_opens_for_the_whole_opd() {
    let d = doc(&blind(7, &[]));
    let mut sim = Simulator::new(&d);
    sim.check("p7-OutputPos").unwrap();
    let state = |sim: &Simulator| {
        (
            sim.read("p7-OutputUp"),
            sim.read("p7-OutputDown"),
            (sim.read("p7-OutputPos") * 100.0).round() / 100.0,
        )
    };
    pulse(&mut sim, "p2-AQ");
    assert_eq!(state(&sim), (0.0, 1.0, 0.0));
    sim.tick(25.0);
    assert_eq!(state(&sim), (0.0, 1.0, 0.5));
    // Cc while moving stops.
    pulse(&mut sim, "p2-AQ");
    sim.tick(10.0);
    assert_eq!(state(&sim), (0.0, 0.0, 0.5));
    // Cc again: only the remaining half of Cld.
    pulse(&mut sim, "p2-AQ");
    sim.tick(24.0);
    assert_eq!(state(&sim).1, 1.0);
    sim.tick(2.0);
    assert_eq!(state(&sim), (0.0, 0.0, 1.0));
    pulse(&mut sim, "p1-AQ");
    sim.tick(59.0);
    assert_eq!(state(&sim), (1.0, 0.0, 0.02));
    sim.tick(1.0);
    assert_eq!(state(&sim), (0.0, 0.0, 0.0));
    // Co runs the whole Opd even from open: a reference run.
    pulse(&mut sim, "p1-AQ");
    sim.tick(59.0);
    assert_eq!(state(&sim), (1.0, 0.0, 0.0));
    sim.tick(1.0);
    assert_eq!(state(&sim), (0.0, 0.0, 0.0));
}

#[test]
fn blind_wind_alarm_window_and_lock() {
    let d = doc(&blind(7, &["SO=1"]));
    let mut sim = Simulator::new(&d);
    // Wind alarm with Wap 1: close completely, and lock.
    sim.set("p4-AQ", 1.0);
    sim.tick(0.1);
    assert_eq!(
        (sim.read("p7-OutputDown"), sim.read("p7-OutputSafety")),
        (1.0, 1.0)
    );
    sim.tick(60.0);
    pulse(&mut sim, "p1-AQ");
    sim.tick(1.0);
    assert_eq!(sim.read("p7-OutputUp"), 0.0, "Co is locked out");
    assert_eq!(sim.read("p7-OutputPos"), 1.0);
    // The end of Wa moves nothing.
    sim.set("p4-AQ", 0.0);
    sim.tick(1.0);
    assert_eq!(
        (sim.read("p7-OutputUp"), sim.read("p7-OutputSafety")),
        (0.0, 0.0)
    );
    // Off stops and dominates, also over the window contact.
    pulse(&mut sim, "p1-AQ");
    sim.tick(1.0);
    sim.set("p6-AQ", 1.0);
    sim.set("p5-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(
        (sim.read("p7-OutputUp"), sim.read("p7-OutputLock")),
        (0.0, 1.0)
    );
    sim.set("p6-AQ", 0.0);
    sim.tick(1.0);
    let err = sim.check("p7-OutputPos").unwrap_err();
    assert!(err.contains("Off ends while Wa or Dwc is on"), "{err}");
}

#[test]
fn blind_window_opens_and_slightly_open_moves_to_rd() {
    let d = doc(&blind(7, &[]));
    let mut sim = Simulator::new(&d);
    pulse(&mut sim, "p3-AQ");
    sim.tick(30.0);
    assert_eq!(sim.read("p7-OutputDown"), 0.0);
    assert!((sim.read("p7-OutputPos") - 0.6).abs() < 1e-9);
    sim.set("p5-AQ", 1.0);
    sim.tick(0.1);
    assert_eq!(sim.read("p7-OutputUp"), 1.0);
    sim.set("p5-AQ", 0.0);
    sim.tick(0.1);
    let err = sim.check("p7-OutputUp").unwrap_err();
    assert!(err.contains("Dwc ends"), "{err}");
}

#[test]
fn blind_refuses_its_unmodelled_inputs_and_outputs() {
    let d = doc(&(vin(8) + &blind(7, &["InputTrigger<p8-AQ"])));
    let mut sim = Simulator::new(&d);
    let err = sim.check("p7-OutputLPos").unwrap_err();
    assert!(
        err.contains("output `OutputLPos` is not simulated"),
        "{err}"
    );
    sim.tick(1.0);
    sim.check("p7-OutputPos").unwrap();
    sim.set("p8-AQ", 1.0);
    sim.tick(1.0);
    let err = sim.check("p7-OutputPos").unwrap_err();
    assert!(
        err.contains("its input Tg (`InputTrigger`) is not simulated"),
        "{err}"
    );
}

/// A `LightController2` `o<n>` with a motion mood setting AQ1 to `q1`
/// and AQ2 to 40 %, Move/Brightness/Reset wired from virtual inputs 1…3.
fn lights(n: u32, q1: u32, extra: &[&str]) -> String {
    let mut ports = vec![
        "Move<p1-AQ",
        "Brightness<p2-AQ",
        "Reset<p3-AQ",
        "BrightnessLimit=30",
        "MoveOn=60",
        "MoveTimeout=3600",
        "MoveIgnore=300",
        "AQ1",
        "AQ2",
        "OutputReset",
    ];
    ports.extend_from_slice(extra);
    let block = obj(n, "LightController2", &ports);
    let moods = format!(
        "<LightscenesC FC=\"0\" Outputs=\"18\" Num=\"2\">\r\n\
         <LightsceneC Name=\"Viel Licht\" UUID=\"m1\" Move=\"true\" Outputs=\"18\" SID=\"777\" \
         CID=\"22\" Q1=\"{q1}\" Q2=\"40\"/>\r\n\
         <LightsceneC Name=\"Aus\" UUID=\"m2\" ReadOnly=\"true\" Outputs=\"18\" SID=\"778\" \
         CID=\"11\" Q1=\"0\" Q2=\"0\"/>\r\n\
         </LightscenesC>\r\n\
         <LSConfig OpM=\"778\" ScMv=\"777\" ScAc=\"1\"/>\r\n</C>\r\n"
    );
    (1..=3).map(vin).collect::<String>() + &block.replace("</C>\r\n", &moods)
}

#[test]
fn lights_follow_motion_with_the_motion_mood() {
    let d = doc(&lights(4, 100, &[]));
    let mut sim = Simulator::new(&d);
    sim.check("p4-AQ1").unwrap();
    sim.set("p2-AQ", 50.0);
    sim.set("p1-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p4-AQ1"), 0.0, "too bright");
    sim.set("p2-AQ", 10.0);
    sim.tick(1.0);
    assert_eq!((sim.read("p4-AQ1"), sim.read("p4-AQ2")), (100.0, 40.0));
    sim.set("p1-AQ", 0.0);
    sim.tick(1.0);
    sim.tick(59.0);
    assert_eq!(sim.read("p4-AQ1"), 100.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p4-AQ1"), 0.0, "off Moet after the falling edge");
}

#[test]
fn lights_off_resets_locks_and_holds_the_automatic_back() {
    let d = doc(&lights(4, 100, &[]));
    let mut sim = Simulator::new(&d);
    pulse(&mut sim, "p1-AQ");
    sim.tick(1.0);
    assert_eq!(sim.read("p4-AQ1"), 100.0);
    sim.set("p3-AQ", 1.0);
    sim.tick(0.1);
    assert_eq!((sim.read("p4-AQ1"), sim.read("p4-OutputReset")), (0.0, 1.0));
    sim.set("p3-AQ", 0.0);
    sim.tick(0.1);
    // Motion within Pto after the manual off is ignored and restarts it.
    sim.tick(200.0);
    pulse(&mut sim, "p1-AQ");
    sim.tick(250.0);
    assert_eq!(sim.read("p4-AQ1"), 0.0);
    sim.tick(60.0);
    pulse(&mut sim, "p1-AQ");
    assert_eq!(sim.read("p4-AQ1"), 100.0);
}

#[test]
fn lights_refuse_ambiguous_moods_and_unmodelled_inputs() {
    // A switch circuit's `on` (stored 1) reads 1.
    let d = doc(&lights(4, 1, &[]));
    let mut sim = Simulator::new(&d);
    pulse(&mut sim, "p1-AQ");
    assert_eq!(sim.read("p4-AQ1"), 1.0);
    let d = doc(&(vin(5) + &lights(4, 100, &["I1<p5-AQ"])));
    let mut sim = Simulator::new(&d);
    sim.set("p5-AQ", 1.0);
    sim.tick(1.0);
    let err = sim.check("p4-AQ1").unwrap_err();
    assert!(
        err.contains("its input Lc1 (`I1`) is not simulated"),
        "{err}"
    );
    let d = doc(&obj(4, "LightController2", &["AQ1"]));
    let err = Simulator::new(&d).check("p4-AQ1").unwrap_err();
    assert!(err.contains("no moods yet"), "{err}");
}

/// A `LightController2` `o20` with the moods "Viel Licht" (777, motion),
/// A (1, AQ1 20 %), B (2, 40 %), C (3, 60 %) and off, in that order — the
/// probe of oracle session 35 — and M+ (p11), M− (p12), T5/1 (p13),
/// T5/2 (p14), On (p15), Buzzer (p16), Off (p17), Mo (p18) wired.
fn mood_lights(extra: &[&str]) -> String {
    let mut ports = vec![
        "InputTriggerUp<p11-AQ",
        "InputTriggerDown<p12-AQ",
        "Sel1<p13-AQ",
        "Sel2<p14-AQ",
        "On<p15-AQ",
        "AlarmClock<p16-AQ",
        "Reset<p17-AQ",
        "Move<p18-AQ",
        "MoveOn=60",
        "MoveTimeout=3600",
        "MoveIgnore=300",
        "AQ1",
        "Scene",
    ];
    ports.extend_from_slice(extra);
    let block = obj(20, "LightController2", &ports);
    let mood = |name: &str, sid: &str, q1: u32, flag: &str| {
        format!(
            "<LightsceneC Name=\"{name}\" UUID=\"m{sid}\"{flag} Outputs=\"18\" SID=\"{sid}\" \
             CID=\"1\" Q1=\"{q1}\"/>\r\n"
        )
    };
    let moods = format!(
        "<LightscenesC FC=\"0\" Outputs=\"18\" Num=\"5\">\r\n{}{}{}{}{}</LightscenesC>\r\n\
         <LSConfig OpM=\"778\" ScMv=\"777\" ScAc=\"1\"/>\r\n</C>\r\n",
        mood("Viel Licht", "777", 100, " Move=\"true\""),
        mood("A", "1", 20, ""),
        mood("B", "2", 40, ""),
        mood("C", "3", 60, ""),
        mood("Aus", "778", 0, " ReadOnly=\"true\""),
    );
    (11..=18).map(vin).collect::<String>() + &block.replace("</C>\r\n", &moods)
}

/// A click on `port`, then a second of quiet.
fn click(sim: &mut Simulator, port: &str) -> (f64, f64) {
    pulse(sim, port);
    sim.tick(1.0);
    (sim.read("p20-Scene"), sim.read("p20-AQ1"))
}

#[test]
fn lights_step_through_the_mood_list_with_m_plus_and_m_minus() {
    let d = doc(&mood_lights(&[]));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    let ups: Vec<_> = (0..6).map(|_| click(&mut sim, "p11-AQ")).collect();
    assert_eq!(
        ups,
        [
            (99.0, 100.0),
            (1.0, 20.0),
            (2.0, 40.0),
            (3.0, 60.0),
            (0.0, 0.0),
            (99.0, 100.0)
        ],
        "list order, the off mood a step, around the end"
    );
    let downs: Vec<_> = (0..5).map(|_| click(&mut sim, "p12-AQ").0).collect();
    assert_eq!(downs, [0.0, 3.0, 2.0, 1.0, 99.0]);
    // After Off, M+ goes on from the off mood's place.
    click(&mut sim, "p11-AQ");
    click(&mut sim, "p17-AQ");
    assert_eq!(click(&mut sim, "p11-AQ").0, 99.0);
}

#[test]
fn lights_t5_selects_its_mood_then_steps_on() {
    let d = doc(&mood_lights(&[]));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    let t1: Vec<_> = (0..5).map(|_| click(&mut sim, "p13-AQ").0).collect();
    assert_eq!(t1, [1.0, 2.0, 3.0, 0.0, 99.0]);
    // Another input made the last change: T5/2 selects its mood …
    assert_eq!(click(&mut sim, "p14-AQ").0, 2.0);
    assert_eq!(click(&mut sim, "p14-AQ").0, 3.0);
    // … T5/1 after M+ selects its own …
    click(&mut sim, "p11-AQ");
    assert_eq!(click(&mut sim, "p13-AQ").0, 1.0);
    // … and T5/2 while its mood shows steps on.
    assert_eq!(click(&mut sim, "p13-AQ").0, 2.0);
    assert_eq!(click(&mut sim, "p14-AQ").0, 3.0);
}

#[test]
fn lights_on_and_buzzer_select_the_bright_mood() {
    let d = doc(&mood_lights(&[]));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    click(&mut sim, "p14-AQ");
    assert_eq!(click(&mut sim, "p15-AQ").0, 99.0);
    click(&mut sim, "p17-AQ");
    // Buzzer without an alarm-clock mood: ID 99, off Met after it falls.
    assert_eq!(click(&mut sim, "p16-AQ").0, 99.0);
    sim.tick(3000.0);
    sim.tick(598.0);
    assert_eq!(sim.read("p20-Scene"), 99.0);
    sim.tick(2.0);
    assert_eq!((sim.read("p20-Scene"), sim.read("p20-AQ1")), (0.0, 0.0));
    // DisPc disables the pulse inputs.
    let d = doc(&(vin(19) + &mood_lights(&["InputDisable<p19-AQ"])));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    sim.set("p19-AQ", 1.0);
    assert_eq!(click(&mut sim, "p11-AQ").0, 0.0);
}

#[test]
fn lights_refuse_unverified_manual_operation() {
    let refused = |steps: &dyn Fn(&mut Simulator)| {
        let d = doc(&mood_lights(&[]));
        let mut sim = Simulator::new(&d);
        sim.check("p20-Scene").unwrap();
        steps(&mut sim);
        sim.check("p20-Scene").unwrap_err()
    };
    let err = refused(&|sim| {
        pulse(sim, "p11-AQ");
        sim.tick(0.1);
        pulse(sim, "p11-AQ");
    });
    assert!(err.contains("a double click on M+"), "{err}");
    let err = refused(&|sim| {
        sim.set("p13-AQ", 1.0);
        sim.tick(0.1);
        sim.tick(1.0);
    });
    assert!(err.contains("T5/1 held for 1 s"), "{err}");
    let err = refused(&|sim| {
        click(sim, "p13-AQ");
        sim.tick(31.0);
        click(sim, "p13-AQ");
    });
    assert!(err.contains("more than 30 s"), "{err}");
    let err = refused(&|sim| {
        for _ in 0..4 {
            click(sim, "p13-AQ");
        }
        click(sim, "p18-AQ");
    });
    assert!(err.contains("switched off by M+, M−, T5 or Mood"), "{err}");
    let err = refused(&|sim| {
        click(sim, "p18-AQ");
        click(sim, "p11-AQ");
    });
    assert!(err.contains("wait to go off"), "{err}");
    let err = refused(&|sim| {
        sim.set("p15-AQ", 1.0);
        sim.tick(1.0);
        click(sim, "p11-AQ");
    });
    assert!(err.contains("while On or Buzzer is held"), "{err}");
}

#[test]
fn lights_m_plus_and_t5_act_on_release_on_acts_on_press() {
    let d = doc(&mood_lights(&[]));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    sim.set("p11-AQ", 1.0);
    sim.tick(0.1);
    sim.tick(2.0);
    assert_eq!(sim.read("p20-Scene"), 0.0, "M+ held");
    sim.set("p11-AQ", 0.0);
    sim.tick(0.1);
    assert_eq!(sim.read("p20-Scene"), 99.0);
    click(&mut sim, "p17-AQ");
    sim.set("p15-AQ", 1.0);
    sim.tick(0.1);
    assert_eq!(sim.read("p20-Scene"), 99.0, "On held");
}

#[test]
fn lights_manual_moods_and_motion_go_off_met_after_motion() {
    let d = doc(&mood_lights(&["MoveTimeout=20"]));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    // A mood chosen by hand stays without motion, Met or not.
    click(&mut sim, "p13-AQ");
    sim.tick(100.0);
    assert_eq!(sim.read("p20-Scene"), 1.0);
    // Motion keeps it; Met (not Moet) after motion ends it goes off.
    click(&mut sim, "p18-AQ");
    assert_eq!(sim.read("p20-Scene"), 1.0);
    sim.tick(19.0);
    assert_eq!(sim.read("p20-Scene"), 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p20-Scene"), 0.0);
    // A mood chosen during motion goes off Met after it too.
    sim.set("p18-AQ", 1.0);
    sim.tick(1.0);
    assert_eq!(sim.read("p20-Scene"), 99.0);
    click(&mut sim, "p14-AQ");
    assert_eq!(sim.read("p20-Scene"), 2.0);
    sim.set("p18-AQ", 0.0);
    sim.tick(0.1);
    sim.tick(19.8);
    assert_eq!(sim.read("p20-Scene"), 2.0);
    sim.tick(0.2);
    assert_eq!(sim.read("p20-Scene"), 0.0);
}

#[test]
fn lights_mood_selects_by_id_when_it_changes() {
    let d = doc(&(vin(19) + &mood_lights(&["Select<p19-AQ"])));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    sim.tick(0.5);
    let mut select = |v: f64| {
        sim.set("p19-AQ", v);
        sim.tick(0.5);
        sim.read("p20-Scene")
    };
    let seen: Vec<f64> = [2.0, 0.0, 777.0, 778.0, 99.0, 1.0, 4.0, 3.0, 102.0]
        .into_iter()
        .map(&mut select)
        .collect();
    assert_eq!(seen, [2.0, 0.0, 99.0, 0.0, 99.0, 1.0, 99.0, 3.0, 99.0]);
    // A constant selects nothing.
    let d = doc(&mood_lights(&["Select=2"]));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    sim.tick(1.0);
    assert_eq!(sim.read("p20-Scene"), 0.0);
}

#[test]
fn lights_alarm_flashes_every_circuit_and_returns_to_the_mood() {
    let alarm = ["Alarm<p19-AQ", "AlarmPeriod=10", "AlarmBrightness=70"];
    let d = doc(&(vin(19) + &mood_lights(&alarm)));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    click(&mut sim, "p13-AQ");
    sim.set("p19-AQ", 1.0);
    let seen: Vec<(f64, f64)> = (0..5)
        .map(|_| {
            sim.tick(2.5);
            (sim.read("p20-Scene"), sim.read("p20-AQ1"))
        })
        .collect();
    assert_eq!(
        seen,
        [
            (99.0, 70.0),
            (99.0, 70.0),
            (0.0, 0.0),
            (0.0, 0.0),
            (99.0, 70.0)
        ],
        "Afi 10: 5 s on, 5 s off"
    );
    sim.set("p19-AQ", 0.0);
    sim.tick(0.1);
    assert_eq!((sim.read("p20-Scene"), sim.read("p20-AQ1")), (1.0, 20.0));
    // Without Afi and MaxAbr on the block Config flashed at another rate.
    let d = doc(&(vin(19) + &mood_lights(&["Alarm<p19-AQ"])));
    let mut sim = Simulator::new(&d);
    sim.check("p20-Scene").unwrap();
    sim.set("p19-AQ", 1.0);
    sim.tick(0.1);
    let err = sim.check("p20-Scene").unwrap_err();
    assert!(err.contains("Alarm without Afi and MaxAbr"), "{err}");
}
