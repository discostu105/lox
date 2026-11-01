//! Virtual and Modbus devices from source (D64): minting them into their
//! folders and devices, the `device:` binding's checks, and adopting an
//! existing device with its I/O — promoting the externs the source has
//! on them.

use loxone_config::logic::{CompileOptions, Module, adopt_one, compile};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

/// A config with the folders the periphery lives in and one logic page.
fn base() -> LoxoneDoc {
    LoxoneDoc::parse(
        "<ControlList Version=\"1\" NextObj=\"50\">\r\n\
         \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\" ConfigVersion=\"17020828\">\r\n\
         \t\t<C Type=\"VirtualInCaption\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"Virtuelle Eingänge\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"VirtualOutCaption\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Virtuelle Ausgänge\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"WeatherCaption\" U=\"00000004-0000-0000-ffff000000000001\" Title=\"Netzwerkperipherie\">\r\n\
         \t\t</C>\r\n\
         \t\t<C Type=\"Page\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Regeln\">\r\n\
         \t\t</C>\r\n\
         \t</C>\r\n\
         </ControlList>\r\n"
            .as_bytes(),
    )
    .unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Regeln".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

const DEVICES: &str = r#"
http = VirtualHttpIn("Zähler", Address: "http://192.0.2.10/status", Poll: 600)
http_power = VirtualHttpInCmd("Leistung", device: http, Check: "\"power\":\\v")
udp = VirtualUdpIn("Kamera", Port: 45999)
udp_person = VirtualUdpInCmd("Person", device: udp, Check: "person:\\v")
text = VirtualTextIn("Status")
out = VirtualOut("Aktor", Address: "http://192.0.2.11")
out_cmd = VirtualOutCmd("Ein", device: out, CmdOn: "/on")
mb = ModbusServer("Wärmepumpe", Address: "192.0.2.12:502")
mb_dev = ModbusDev("Regler", device: mb)
mb_temp = ModbusASensor("Vorlauf", device: mb_dev, ModbusAddress: 1, ModbusPollingCycle: 600)
mb_alarm = ModbusSensor("Alarm", device: mb_dev, ModbusAddress: 2)
mb_set = ModbusAActor("Sollwert", device: mb_dev, ModbusAddress: 3, I: plus.AQ)

page "Regeln"
plus = Formula(Formula: "I1+I2", Input1: http_power.AQ, Input2: mb_temp.Q)
"#;

fn devices() -> Module {
    Module::parse(DEVICES).unwrap()
}

fn element<'a>(doc: &'a LoxoneDoc, uuid: &str) -> &'a loxone_config::xml::Element {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    // `objects()` borrows `doc`; the element lives as long as `doc`.
    doc.element_at(&o.path).unwrap()
}

fn parent_type(doc: &LoxoneDoc, uuid: &str) -> String {
    let o = doc.objects().into_iter().find(|o| o.uuid == uuid).unwrap();
    doc.element_at(&o.path[..o.path.len() - 1])
        .unwrap()
        .attr("Type")
        .unwrap()
        .to_string()
}

#[test]
fn devices_are_minted_into_their_folders_and_converge() {
    let base = base();
    let module = devices();
    let mut lock = Lockfile::default();
    let out = compile(&base, &module, &mut lock, &opts()).unwrap();
    let uuid = |s: &str| lock.objects[s].uuid.clone();

    for (slug, parent) in [
        ("http", "VirtualInCaption"),
        ("http_power", "VirtualHttpIn"),
        ("udp", "VirtualInCaption"),
        ("udp_person", "VirtualUdpIn"),
        ("text", "VirtualInCaption"),
        ("out", "VirtualOutCaption"),
        ("out_cmd", "VirtualOut"),
        ("mb", "WeatherCaption"),
        ("mb_dev", "ModbusServer"),
        ("mb_temp", "ModbusDev"),
        ("mb_alarm", "ModbusDev"),
        ("mb_set", "ModbusDev"),
    ] {
        assert_eq!(parent_type(&out, &uuid(slug)), parent, "{slug}");
    }
    // Designations per prefix; the HTTP and UDP commands share `VCI`.
    for (slug, iname) in [
        ("http", "VHI1"),
        ("http_power", "VCI1"),
        ("udp", "VUI1"),
        ("udp_person", "VCI2"),
        ("text", "VTI1"),
        ("out", "VQ1"),
        ("out_cmd", "VQC1"),
        ("mb_temp", "AMQ1"),
        ("mb_set", "AMQ2"),
    ] {
        assert_eq!(
            element(&out, &uuid(slug)).attr("IName"),
            Some(iname),
            "{slug}"
        );
    }
    let http = element(&out, &uuid("http"));
    assert_eq!(http.attr("Address"), Some("http://192.0.2.10/status"));
    assert_eq!(http.attr("Poll"), Some("600"));
    // `\\v` in source is Loxone's `\v` placeholder.
    assert_eq!(
        element(&out, &uuid("http_power"))
            .attr_decoded("Check")
            .as_deref(),
        Some("\"power\":\\v")
    );

    // The wire from the page into the Modbus actuator, and from the
    // commands into the formula.
    let wires = out.wires();
    let port = |s: &str, k: &str| lock.objects[s].ports[k].clone();
    for (from, to) in [
        (port("plus", "AQ"), port("mb_set", "I")),
        (port("http_power", "AQ"), port("plus", "Input1")),
        (port("mb_temp", "Q"), port("plus", "Input2")),
    ] {
        assert!(
            wires.iter().any(|w| w.from_port == from && w.to_port == to),
            "{from} -> {to}"
        );
    }

    // A rebuild of its own output is byte-identical.
    let mut lock2 = lock.clone();
    let again = compile(&out, &module, &mut lock2, &opts()).unwrap();
    assert_eq!(again.to_bytes(), out.to_bytes());
    assert_eq!(lock2.objects, lock.objects);
    assert_eq!(lock2.counters, lock.counters);
}

#[test]
fn device_binding_is_checked() {
    let base = base();
    let refuse = |src: &str, want: &str| {
        let err = Module::parse(src)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                m.validate().map_err(|e| e.to_string())?;
                compile(&base, &m, &mut Lockfile::default(), &opts())
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .unwrap_err();
        assert!(err.contains(want), "{src}: {err}");
    };
    refuse("c = VirtualHttpInCmd(\"C\", Check: \"x\")\n", "device:");
    refuse(
        "u = VirtualUdpIn(\"U\", Port: 1)\nc = VirtualHttpInCmd(\"C\", device: u, Check: \"x\")\n",
        "VirtualHttpIn",
    );
    refuse(
        "f = Formula(Formula: \"I1\")\nc = VirtualHttpInCmd(\"C\", device: f, Check: \"x\")\n",
        "VirtualHttpIn",
    );
    refuse("t = VirtualTextIn(\"T\", device: t)\n", "device");
    // A new device needs what makes it work.
    refuse("h = VirtualHttpIn(\"H\", Poll: 10)\n", "needs `Address:`");
    refuse(
        "m = ModbusServer(\"M\", Address: \"192.0.2.1:502\")\nd = ModbusDev(\"D\", device: m)\n\
         s = ModbusASensor(\"S\", device: d)\n",
        "needs `ModbusAddress:`",
    );
}

#[test]
fn adopting_a_device_takes_its_io_and_promotes_externs() {
    // A config where the devices exist and the source only references
    // them: mint, then forget the lock.
    let mut lock = Lockfile::default();
    let made = compile(&base(), &devices(), &mut lock, &opts()).unwrap();
    let uuid = |s: &str| lock.objects[s].uuid.clone();
    let referencing = Module::parse(
        "extern leistung = VirtualHttpInCmd(title: \"Leistung\")\n\
         extern vorlauf = ModbusASensor(title: \"Vorlauf\")\n\
         extern sollwert = ModbusAActor(title: \"Sollwert\")\n\
         page \"Regeln\"\n\
         plus = Formula(Formula: \"I1+I2\", Input1: leistung.AQ, Input2: vorlauf.Q)\n\
         sollwert.I <- plus.AQ\n",
    )
    .unwrap();
    let mut lock1 = Lockfile::default();
    lock1
        .objects
        .insert("plus".into(), lock.objects["plus"].clone());
    let base1 = compile(&made, &referencing, &mut lock1, &opts()).unwrap();
    assert_eq!(base1.to_bytes(), made.to_bytes());

    // Adopting the HTTP device takes its command along — it was extern
    // `leistung`, which keeps its slug.
    let mut module = referencing.clone();
    let mut lock2 = lock1.clone();
    let adopted = adopt_one(&base1, &uuid("http"), "zaehler", &module, &mut lock2).unwrap();
    assert_eq!(adopted.promoted, ["leistung"]);
    assert!(!adopted.on_page);
    assert_eq!(adopted.page_title, "Virtuelle Eingänge");
    assert!(module.strip_extern("leistung"));
    module.items.extend(adopted.items);
    let text = module.to_text();
    assert!(
        text.contains("leistung = VirtualHttpInCmd(\n\t\"Leistung\",\n\tdevice: zaehler,"),
        "{text}"
    );
    // Text-boolean settings read `on`/`off` (D80).
    assert!(text.contains("\tSigned: on,\n\tAnalog: on,\n"), "{text}");
    let out = compile(&base1, &module, &mut lock2, &opts()).unwrap();
    assert_eq!(out.to_bytes(), base1.to_bytes());

    // The Modbus server: the device in between gets a slug of its own,
    // the actuator's wire from the page moves into its declaration.
    let adopted = adopt_one(&base1, &uuid("mb"), "wp", &module, &mut lock2).unwrap();
    assert_eq!(adopted.promoted, ["vorlauf", "sollwert"]);
    for x in &adopted.promoted {
        assert!(module.strip_extern(x));
    }
    module.items.extend(adopted.items);
    let text = module.to_text();
    assert!(
        text.contains("wp_regler = ModbusDev(\"Regler\", device: wp"),
        "{text}"
    );
    assert!(text.contains("I: plus.AQ"), "{text}");
    assert!(!text.contains("<-"), "{text}");
    let out = compile(&base1, &module, &mut lock2, &opts()).unwrap();
    assert_eq!(out.to_bytes(), base1.to_bytes());

    // Once managed, an extern naming one of them is refused.
    let mut again = module.clone();
    again.items.extend(
        Module::parse("extern alt = ModbusASensor(title: \"Vorlauf\")\n")
            .unwrap()
            .items,
    );
    let err = compile(&base1, &again, &mut lock2.clone(), &opts()).unwrap_err();
    assert!(err.to_string().contains("manages as `vorlauf`"), "{err}");
}

#[test]
fn removing_a_device_and_an_io_converges_against_the_old_base() {
    // Mint everything, then withdraw the HTTP device (with its command)
    // and the Modbus actuator from a device that stays.
    let mut lock = Lockfile::default();
    let deployed = compile(&base(), &devices(), &mut lock, &opts()).unwrap();
    let gone: Vec<String> = ["http", "http_power", "mb_set"]
        .iter()
        .map(|s| lock.objects[*s].uuid.clone())
        .collect();
    let src = DEVICES
        .lines()
        .filter(|l| !l.starts_with("http") && !l.starts_with("mb_set"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("Input1: http_power.AQ, ", "")
        + "\nremoved http\nremoved http_power\nremoved mb_set\n";
    let module = Module::parse(&src).unwrap();
    let out = compile(&deployed, &module, &mut lock, &opts()).unwrap();
    let has = |d: &LoxoneDoc, u: &str| d.objects().iter().any(|o| o.uuid == u);
    assert!(gone.iter().all(|u| !has(&out, u)));
    assert_eq!(lock.removed.len(), 3);

    // Until the removal is deployed, every compile against the old base
    // removes them again — none comes back with the device that held it.
    let mut again = lock.clone();
    let out2 = compile(&deployed, &module, &mut again, &opts()).unwrap();
    assert_eq!(out2.to_bytes(), out.to_bytes());
    assert_eq!(again.removed, lock.removed);
    // Against the deployed result, the tombstones are done.
    let out3 = compile(&out, &module, &mut again, &opts()).unwrap();
    assert_eq!(out3.to_bytes(), out.to_bytes());
    assert!(again.removed.is_empty());
}

#[test]
fn a_virtual_input_is_adopted_in_its_place() {
    // Three virtual inputs; another project names the middle one.
    let three = Module::parse(
        "a = VirtualIn(\"Eingang A\")\nb = VirtualIn(\"Eingang B\")\nc = VirtualIn(\"Eingang C\")\n",
    )
    .unwrap();
    let mut lock1 = Lockfile::default();
    let base1 = compile(&base(), &three, &mut lock1, &opts()).unwrap();
    let base1 = LoxoneDoc::parse(&base1.to_bytes()).unwrap();
    let b = lock1.objects["b"].uuid.clone();

    let mut module = Module::parse("extern eingang = VirtualIn(title: \"Eingang B\")\n").unwrap();
    let mut lock = Lockfile::default();
    compile(&base1, &module, &mut lock, &opts()).unwrap();
    let adopted = adopt_one(&base1, &b, "eingang", &module, &mut lock).unwrap();
    assert_eq!(adopted.promoted, ["eingang"]);
    assert!(!adopted.on_page);
    assert_eq!(adopted.page_title, "Virtuelle Eingänge");
    assert!(module.strip_extern("eingang"));
    module.items.extend(adopted.items);
    assert!(
        module
            .to_text()
            .contains("eingang = VirtualIn(\"Eingang B\")"),
        "{}",
        module.to_text()
    );
    // Rebuilt where it was, between the other two.
    let out = compile(&base1, &module, &mut lock, &opts()).unwrap();
    assert!(out.to_bytes() == base1.to_bytes());
    assert_eq!(lock.objects["eingang"].uuid, b);
    assert!(lock.objects["eingang"].page_uuid.is_none());
}

#[test]
fn off_is_no_attribute_except_on_a_virtual_input() {
    // A save drops `Signed="false"`, `Analog="false"` and
    // `CloseAfterSend="false"` on the virtual commands and outputs, and
    // writes `Analog="false"` on a digital VirtualIn (#298).
    let src = |cmd: &str| {
        format!(
            "vi = VirtualIn(\"Taster\", Analog: off)\n\
             out = VirtualOut(\"Aktor\", Address: \"http://192.0.2.11\", CloseAfterSend: off)\n\
             out_cmd = VirtualOutCmd(\"Ein\", device: out, CmdOn: \"/on\", Analog: {cmd})\n\
             udp = VirtualUdpIn(\"Kamera\", Port: 45999)\n\
             udp_cmd = VirtualUdpInCmd(\"Person\", device: udp, Check: \"p\", Signed: off, Analog: off)\n\
             http = VirtualHttpIn(\"Zähler\", Address: \"http://192.0.2.10/s\", Poll: 600)\n\
             http_cmd = VirtualHttpInCmd(\"Leistung\", device: http, Check: \"p\", Signed: off, Analog: off)\n"
        )
    };
    let mut lock = Lockfile::default();
    let module = Module::parse(&src("on")).unwrap();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let attr = |doc: &LoxoneDoc, slug: &str, name: &str| {
        element(doc, &lock.objects[slug].uuid)
            .attr(name)
            .map(str::to_string)
    };
    assert_eq!(attr(&out, "vi", "Analog").as_deref(), Some("false"));
    assert_eq!(attr(&out, "out", "CloseAfterSend"), None);
    assert_eq!(attr(&out, "out_cmd", "Analog").as_deref(), Some("true"));
    for slug in ["udp_cmd", "http_cmd"] {
        assert_eq!(attr(&out, slug, "Signed"), None, "{slug}");
        assert_eq!(attr(&out, slug, "Analog"), None, "{slug}");
    }
    // Switching a ticked box off removes the attribute; the result is
    // stable.
    let module = Module::parse(&src("off")).unwrap();
    let mut lock2 = lock.clone();
    let off = compile(&out, &module, &mut lock2, &opts()).unwrap();
    assert_eq!(attr(&off, "out_cmd", "Analog"), None);
    let again = compile(&off, &module, &mut lock2.clone(), &opts()).unwrap();
    assert_eq!(again.to_bytes(), off.to_bytes());
}

#[test]
fn zero_is_no_attribute_on_a_number() {
    // A save drops every numeric setting at 0, except a Modbus server's
    // `Timeout="0"` (#312, Loxone Config 17.2).
    let src = |min: &str| {
        format!(
            "vi = VirtualIn(\"Wert\", Analog: on, MinVal: {min}, MaxVal: 0, DefVal: 0)\n\
             http = VirtualHttpIn(\"Zähler\", Address: \"http://192.0.2.10/s\", Poll: 600)\n\
             http_cmd = VirtualHttpInCmd(\"Leistung\", device: http, Check: \"p\", MinVal: 0, SourceValLow: 0)\n\
             mb = ModbusServer(\"Wärmepumpe\", Address: \"192.0.2.12:502\", Timeout: 0)\n\
             mb_dev = ModbusDev(\"Regler\", device: mb, Channel: 0)\n\
             mb_temp = ModbusASensor(\"Vorlauf\", device: mb_dev, ModbusAddress: 0, ModbusDataType: 0, MaxTime: 0)\n"
        )
    };
    let mut lock = Lockfile::default();
    let module = Module::parse(&src("0")).unwrap();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let attr = |doc: &LoxoneDoc, slug: &str, name: &str| {
        element(doc, &lock.objects[slug].uuid)
            .attr(name)
            .map(str::to_string)
    };
    for (slug, name) in [
        ("vi", "MinVal"),
        ("vi", "MaxVal"),
        ("vi", "DefVal"),
        ("http_cmd", "MinVal"),
        ("http_cmd", "SourceValLow"),
        ("mb_dev", "Channel"),
        ("mb_temp", "ModbusAddress"),
        ("mb_temp", "ModbusDataType"),
        ("mb_temp", "MaxTime"),
    ] {
        assert_eq!(attr(&out, slug, name), None, "{slug}.{name}");
    }
    assert_eq!(attr(&out, "mb", "Timeout").as_deref(), Some("0"));
    assert_eq!(attr(&out, "http_cmd", "MaxVal").as_deref(), Some("10000"));

    // Setting a value and back to 0 removes the attribute again.
    let mut lock2 = lock.clone();
    let five = compile(
        &out,
        &Module::parse(&src("5")).unwrap(),
        &mut lock2,
        &opts(),
    )
    .unwrap();
    assert_eq!(attr(&five, "vi", "MinVal").as_deref(), Some("5"));
    let zero = compile(&five, &module, &mut lock2, &opts()).unwrap();
    assert_eq!(attr(&zero, "vi", "MinVal"), None);
    let again = compile(&zero, &module, &mut lock2.clone(), &opts()).unwrap();
    assert_eq!(again.to_bytes(), zero.to_bytes());

    // Adopting states a missing number as 0 where a fresh object would
    // get another default, so the source means the same anywhere.
    let mut fresh = Lockfile::default();
    let adopted = adopt_one(
        &out,
        &lock.objects["mb"].uuid,
        "wp",
        &Module::default(),
        &mut fresh,
    )
    .unwrap();
    let mut adopted_module = Module::default();
    adopted_module.items.extend(adopted.items);
    let text = adopted_module.to_text();
    assert!(text.contains("ModbusDataType: 0"), "{text}");
    assert!(text.contains("MaxTime: 0"), "{text}");
    assert!(text.contains("Channel: 0"), "{text}");
    assert!(text.contains("ModbusAddress: 0"), "{text}");
    let recompiled = compile(&out, &adopted_module, &mut fresh, &opts()).unwrap();
    assert_eq!(recompiled.to_bytes(), out.to_bytes());
}

#[test]
fn adopt_states_an_unticked_box_a_fresh_object_ticks() {
    // An unticked `Signed`/`Analog`/`CloseAfterSend` has no attribute,
    // and a fresh object ticks it: adopt writes `off`, so the source
    // means the same on another config (#333).
    let src = "out = VirtualOut(\"Aktor\", Address: \"http://192.0.2.11\", CloseAfterSend: off)\n\
               out_cmd = VirtualOutCmd(\"Ein\", device: out, CmdOn: \"/on\")\n\
               udp = VirtualUdpIn(\"Kamera\", Port: 45999)\n\
               udp_cmd = VirtualUdpInCmd(\"Person\", device: udp, Check: \"p\", Signed: off, Analog: off)\n\
               http = VirtualHttpIn(\"Zähler\", Address: \"http://192.0.2.10/s\", Poll: 600)\n\
               http_cmd = VirtualHttpInCmd(\"Leistung\", device: http, Check: \"p\", Signed: off)\n";
    let mut lock = Lockfile::default();
    let out = compile(&base(), &Module::parse(src).unwrap(), &mut lock, &opts()).unwrap();
    let mut fresh = Lockfile::default();
    let mut adopted_module = Module::default();
    for (slug, name) in [("out", "aktor"), ("udp", "kamera"), ("http", "zaehler")] {
        let adopted = adopt_one(
            &out,
            &lock.objects[slug].uuid,
            name,
            &adopted_module,
            &mut fresh,
        )
        .unwrap();
        adopted_module.items.extend(adopted.items);
    }
    let text = adopted_module.to_text();
    assert!(text.contains("CloseAfterSend: off,"), "{text}");
    assert_eq!(text.matches("Signed: off,").count(), 2, "{text}");
    assert!(text.contains("Analog: off,"), "{text}");
    assert!(text.contains("Analog: on,"), "{text}");
    // A box a fresh object leaves unticked lifts as nothing.
    assert_eq!(text.matches("Analog:").count(), 2, "{text}");
    let recompiled = compile(&out, &adopted_module, &mut fresh, &opts()).unwrap();
    assert_eq!(recompiled.to_bytes(), out.to_bytes());

    // Minted fresh, the boxes stay unticked.
    let elsewhere = compile(&base(), &adopted_module, &mut Lockfile::default(), &opts()).unwrap();
    for o in elsewhere.objects() {
        let el = elsewhere.element_at(&o.path).unwrap();
        match o.block_type.as_str() {
            "VirtualOut" => assert_eq!(el.attr("CloseAfterSend"), None),
            "VirtualUdpInCmd" => {
                assert_eq!(el.attr("Signed"), None);
                assert_eq!(el.attr("Analog"), None);
            }
            "VirtualHttpInCmd" => {
                assert_eq!(el.attr("Signed"), None);
                assert_eq!(el.attr("Analog"), Some("true"));
            }
            _ => {}
        }
    }
}

#[test]
fn a_value_a_save_rewrites_is_refused() {
    // Loxone Config raises Poll and Timeout to 10, cuts them to whole
    // numbers, and fixes a digital VirtualIn's range at 0 to 1 (#332).
    let base = base();
    let run = |src: &str| {
        Module::parse(src).map_err(|e| e.to_string()).and_then(|m| {
            m.validate().map_err(|e| e.to_string())?;
            loxone_config::logic::validate_ports(&m).map_err(|e| e.to_string())?;
            compile(&base, &m, &mut Lockfile::default(), &opts())
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    };
    let http = |settings: &str| {
        format!("h = VirtualHttpIn(\"H\", Address: \"http://192.0.2.10/s\", {settings})\n")
    };
    for (settings, want) in [
        ("Poll: 5", "`Poll: 10`"),
        ("Poll: 600.5", "`Poll: 600`"),
        ("Poll: 200h", "`Poll: 604800`"),
        ("Poll: 600, Timeout: 9", "`Timeout: 10`"),
        ("Poll: 600, Timeout: 0", "`Timeout: 10`"),
        ("Poll: 600, Timeout: 40.5ms", "`Timeout: 40`"),
        ("Poll: 600, Allowedtimeouts: 2.5", "`Allowedtimeouts: 2`"),
    ] {
        let err = run(&http(settings)).unwrap_err();
        assert!(err.contains(want), "{settings}: {err}");
        assert!(err.contains("a Loxone Config save turns it into"), "{err}");
    }
    for ok in [
        "Poll: 10",
        "Poll: 168h",
        "Poll: 10min, Timeout: 10s",
        "Poll: 600, Allowedtimeouts: -1",
    ] {
        run(&http(ok)).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }

    for (src, want) in [
        ("VirtualIn(\"D\", MaxVal: 5)", "`MaxVal: 1`"),
        ("VirtualIn(\"D\", Analog: off, MaxVal: 0)", "`MaxVal: 1`"),
        ("VirtualIn(\"D\", MinVal: -1)", "`MinVal: 0`"),
    ] {
        let err = run(&format!("d = {src}\n")).unwrap_err();
        assert!(err.contains(want), "{src}: {err}");
    }
    for ok in [
        "VirtualIn(\"D\", MaxVal: 1, MinVal: 0, DefVal: 2)",
        "VirtualIn(\"A\", Analog: on, MinVal: -1, MaxVal: 5)",
    ] {
        run(&format!("d = {ok}\n")).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }

    // Without `Analog:`, an existing analog input stays analog.
    let mut lock = Lockfile::default();
    let analog = compile(
        &base,
        &Module::parse("d = VirtualIn(\"A\", Analog: on)\n").unwrap(),
        &mut lock,
        &opts(),
    )
    .unwrap();
    compile(
        &analog,
        &Module::parse("d = VirtualIn(\"A\", MaxVal: 5)\n").unwrap(),
        &mut lock,
        &opts(),
    )
    .unwrap();
}

#[test]
fn a_title_a_save_rewrites_is_refused() {
    // Loxone Config turns `%`, `<` and `|` into `_`, drops `{` and `}`,
    // and marks spaces at either end (#342).
    let run = |src: &str| {
        Module::parse(src).map_err(|e| e.to_string()).and_then(|m| {
            m.validate().map_err(|e| e.to_string())?;
            loxone_config::logic::validate_ports(&m).map_err(|e| e.to_string())
        })
    };
    for (src, want) in [
        ("d = VirtualIn(\"R Ana Max<Min\")\n", "\"R Ana Max_Min\""),
        ("g = And(\"Licht 100%\")\n", "\"Licht 100_\""),
        ("g = And(\" Licht\")\n", "\"_Licht\""),
        ("m = Mode(\"Urlaub: Juli\")\n", "\"Urlaub_ Juli\""),
        (
            "extern q = Actor(iname: \"Q1\")\nq.title = \"Pumpe {alt}\"\n",
            "\"Pumpe alt\"",
        ),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: {err}");
        assert!(err.contains("a Loxone Config save writes it as"), "{err}");
    }
    let err = run("g = And(\"\")\n").unwrap_err();
    assert!(err.contains("`noname <UUID>`"), "{err}");
    for ok in [
        "d = VirtualIn(\"R Ana Def>Max\")\n",
        "lc = LightController2(\"Licht <EG>\")\n",
        "m = Mode(\"Urlaub < Juli\")\n",
        "mb = ModbusServer(\"Zähler 100%\", Address: \"192.0.2.12:502\")\n",
    ] {
        run(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }
}

#[test]
fn a_udp_command_takes_its_receive_timeout() {
    // Oracle sessions 84 to 86 (#346): the receive timeout `MaxTime` in
    // ms, what the value becomes then (`ValOT="1"` keeps the last value,
    // otherwise `DefVal`), each written where a save writes it and left
    // out at 0.
    let src = "udp = VirtualUdpIn(\"Kamera\", Port: 45998)\n\
         u1 = VirtualUdpInCmd(\"Q U1\", device: udp, Check: \"a\", MaxTime: 7s, DefVal: 3.5)\n\
         u2 = VirtualUdpInCmd(\"Q U2\", device: udp, Check: \"b\", MaxTime: 9s, ValOT: 1)\n\
         u3 = VirtualUdpInCmd(\"Q U3\", device: udp, Check: \"c\", MaxTime: 0, DefVal: 0, ValOT: 0)\n";
    let mut lock = Lockfile::default();
    let module = Module::parse(src).unwrap();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let attrs = |slug: &str| -> Vec<String> {
        element(&out, &lock.objects[slug].uuid)
            .attrs
            .iter()
            .filter(|a| !matches!(a.name.as_str(), "Type" | "U" | "IName" | "V" | "Title"))
            .map(|a| format!("{}={}", a.name, a.value))
            .collect()
    };
    // As session 84's save wrote them.
    let saved = |valot: &str, check: &str, defval: &str, maxtime: &str| -> Vec<String> {
        [
            "Nio=2",
            "WF=16384",
            valot,
            check,
            "Signed=true",
            "Analog=true",
            "SourceValHigh=100",
            "DestValHigh=100",
            defval,
            "MinVal=-10000",
            "MaxVal=10000",
            "MinChange=0.25",
            "MinTime=1000",
            maxtime,
        ]
        .into_iter()
        .filter(|a| !a.is_empty())
        .map(String::from)
        .collect()
    };
    assert_eq!(
        attrs("u1"),
        saved("", "Check=a", "DefVal=3.5", "MaxTime=7000")
    );
    assert_eq!(attrs("u2"), saved("ValOT=1", "Check=b", "", "MaxTime=9000"));
    assert_eq!(attrs("u3"), saved("", "Check=c", "", ""));

    // Adopted, the source names them again, the timeout in seconds.
    let mut fresh = Lockfile::default();
    let adopted = adopt_one(
        &out,
        &lock.objects["udp"].uuid,
        "udp",
        &Module::default(),
        &mut fresh,
    )
    .unwrap();
    let mut adopted_module = Module::default();
    adopted_module.items.extend(adopted.items);
    let text = adopted_module.to_text();
    for want in ["MaxTime: 7s", "DefVal: 3.5", "MaxTime: 9s", "ValOT: 1"] {
        assert!(text.contains(want), "{want}: {text}");
    }
    let recompiled = compile(&out, &adopted_module, &mut fresh, &opts()).unwrap();
    assert_eq!(recompiled.to_bytes(), out.to_bytes());

    // A fraction a save cuts off is refused.
    let err = compile(
        &base(),
        &Module::parse(
            "udp = VirtualUdpIn(\"K\", Port: 45998)\n\
             u = VirtualUdpInCmd(\"U\", device: udp, Check: \"a\", MaxTime: 1.5ms)\n",
        )
        .unwrap(),
        &mut Lockfile::default(),
        &opts(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("`MaxTime: 1`"), "{err}");
}
