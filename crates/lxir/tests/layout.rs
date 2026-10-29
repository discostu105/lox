//! Placement end to end (D42): where compile puts blocks, how it takes
//! over GUI moves, and what `relayout` may move.

use lxir::ir::{CompileOptions, Module, compile};
use lxir::layout::{GAP, GRID, Layout, PageGeometry, ROW, TAG_W};
use lxir::render::metrics;
use lxir::uuid::parse_serial;
use lxir::{Lockfile, LoxoneDoc};

const MINT_TIME: i64 = 1_767_225_600; // 2026-01-01T00:00:00Z
const BESCHATTUNG: &str = "20000004-0000-0040-ffff504f94112233";
const PAGE_BESCHATTUNG: &str =
    "<C Type=\"Page\" V=\"175\" U=\"20000004-0000-0040-ffff504f94112233\" Title=\"Beschattung\">";
const PAGE_WERKSTATT: &str =
    "<C Type=\"Page\" V=\"175\" U=\"20000009-0000-0090-ffff504f94112233\" Title=\"Werkstatt\"></C>";

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/configs/haus.Loxone").unwrap()).unwrap()
}

fn module() -> Module {
    Module::parse(&std::fs::read_to_string("examples/ir/beschattung.lxir").unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: MINT_TIME,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

fn relayout() -> CompileOptions {
    CompileOptions {
        relayout: vec!["Beschattung".into()],
        ..opts()
    }
}

fn uuid(lock: &Lockfile, slug: &str) -> String {
    lock.objects[slug].uuid.clone()
}

fn rect(doc: &LoxoneDoc, uuid: &str) -> Layout {
    let idx = doc.index();
    Layout::of_element(doc.element_at(&idx.by_uuid[uuid]).unwrap()).unwrap()
}

/// `doc` with the object moved to (`x`, `y`), the way the GUI saves a drag.
fn gui_move(doc: &LoxoneDoc, uuid: &str, x: i64, y: i64) -> LoxoneDoc {
    let mut doc = doc.clone();
    let r = rect(&doc, uuid);
    let path = doc.index().by_uuid[uuid].clone();
    let el = doc.element_at_mut(&path).unwrap();
    let to = Layout::at(x, y, r.w(), r.h());
    for (k, v) in [
        ("Px", to.px),
        ("Py", to.py),
        ("Px2", to.px2),
        ("Py2", to.py2),
    ] {
        el.set_attr(k, &v.to_string());
    }
    LoxoneDoc::parse(&doc.to_bytes()).unwrap()
}

#[test]
fn new_blocks_are_placed_clear_on_the_grid_and_left_to_right() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let geo = PageGeometry::of(&out, BESCHATTUNG).unwrap();
    assert!(
        metrics(&geo).overlaps.is_empty(),
        "{:?}",
        metrics(&geo).overlaps
    );

    let temp = rect(&out, &uuid(&lock, "temp_hoch"));
    let and = rect(&out, &uuid(&lock, "beschatten"));
    // The GUI's footprint of a two-input gate, and the chain's direction.
    assert_eq!((and.w(), and.h()), (1344, 696));
    assert!(temp.px2 < and.px, "{temp:?} {and:?}");
    for r in [temp, and] {
        assert_eq!((r.px % GRID, r.py % GRID), (0, 0), "{r:?}");
    }
    // The lock records where they went, and a recompile is a fixpoint.
    assert_eq!(lock.objects["beschatten"].layout, Some(and));
    let again = compile(&out, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}

#[test]
fn a_gui_move_is_taken_over() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let and = uuid(&lock, "beschatten");
    let moved = gui_move(&out, &and, 9600, 4800);

    let out2 = compile(&moved, &module(), &mut lock, &opts()).unwrap();
    let at = rect(&out2, &and);
    assert_eq!((at.px, at.py), (9600, 4800));
    assert_eq!(lock.objects["beschatten"].layout, Some(at));
    assert_eq!(lock.objects["beschatten"].superseded_layout, None);
    let again = compile(&out2, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(out2.to_bytes(), again.to_bytes());
}

#[test]
fn a_relayout_survives_a_stale_base() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let and = uuid(&lock, "beschatten");
    // Dragged far off, downloaded: the stale base.
    let stale = gui_move(&out, &and, 28800, 19200);
    let off = rect(&stale, &and);

    let laid = compile(&stale, &module(), &mut lock, &relayout()).unwrap();
    let fresh = rect(&laid, &and);
    assert_ne!(fresh, off);
    assert_eq!(lock.objects["beschatten"].layout, Some(fresh));
    assert_eq!(lock.objects["beschatten"].superseded_layout, Some(off));

    // Compiling against the stale base again keeps the fresh placement:
    // the old position is the one the relayout replaced, not a GUI move.
    let before = lock.to_json();
    let again = compile(&stale, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(rect(&again, &and), fresh);
    assert_eq!(lock.to_json(), before);

    // Once the base carries it, the guard goes; a later drag is taken over.
    let synced = compile(&laid, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(synced.to_bytes(), laid.to_bytes());
    assert_eq!(lock.objects["beschatten"].superseded_layout, None);
    let dragged = gui_move(&synced, &and, 28800, 19200);
    let out = compile(&dragged, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(rect(&out, &and), off);
}

/// A base whose Beschattung page carries refs nobody in the next lock
/// owns — exactly like GUI-drawn ones: an InputRef mirroring VI1.
fn base_with_gui_ref() -> LoxoneDoc {
    let mirrors = Module::parse(
        "page \"Beschattung\"\n\
         extern quelle = VirtualIn(iname: \"VI1\")\n\
         \n\
         spiegel = InputRef(\n\
         \tmirrors: quelle,\n\
         \tAI: quelle.Q,\n\
         \tI: quelle.Qm,\n\
         )\n\
         \n\
         halter = Memory(\n\
         \tInput: spiegel.Q,\n\
         )\n",
    )
    .unwrap();
    compile(&base(), &mirrors, &mut Lockfile::new(), &opts()).unwrap()
}

fn ref_module() -> Module {
    Module::parse(
        "page \"Beschattung\"\n\
         extern quelle = VirtualIn(iname: \"VI1\")\n\
         \n\
         anzeige = And(\n\
         \tI1: quelle.Q,\n\
         )\n\
         \n\
         folge = And(\n\
         \tI1: anzeige.Q,\n\
         )\n",
    )
    .unwrap()
}

/// The GUI's ref, as an unmanaged object on the compiled page.
fn gui_ref(doc: &LoxoneDoc, lock: &Lockfile) -> String {
    let geo = PageGeometry::of(doc, BESCHATTUNG).unwrap();
    let managed: Vec<&str> = lock.objects.values().map(|e| e.uuid.as_str()).collect();
    geo.objects
        .iter()
        .find(|o| o.block_type == "InputRef" && !managed.contains(&o.uuid.as_str()))
        .unwrap()
        .uuid
        .clone()
}

#[test]
fn a_relayout_keeps_blocks_set_beside_foreign_refs() {
    let module = ref_module();
    let mut lock = Lockfile::new();
    let out = compile(&base_with_gui_ref(), &module, &mut lock, &opts()).unwrap();
    let (anzeige, folge) = (uuid(&lock, "anzeige"), uuid(&lock, "folge"));
    // The GUI drew its ref and the block reading it side by side, far off.
    let spiegel = gui_ref(&out, &lock);
    let moved = gui_move(&out, &spiegel, 19200 - GAP - TAG_W, 19200);
    let moved = gui_move(
        &gui_move(&moved, &anzeige, 19200, 19200),
        &folge,
        38400,
        960,
    );

    let laid = compile(&moved, &module, &mut lock, &relayout()).unwrap();
    // anzeige was arranged with the GUI's ref: it stays where the GUI put it.
    let a = rect(&laid, &anzeige);
    assert_eq!((a.px, a.py), (19200, 19200));
    // folge is ours alone: it moves up beside what feeds it.
    let f = rect(&laid, &folge);
    assert!(f.px > a.px2 && f.px - a.px2 < 4000, "{a:?} {f:?}");
    assert!((f.py - a.py).abs() < 1000, "{a:?} {f:?}");
}

#[test]
fn a_relayout_moves_blocks_far_from_the_ref_they_read() {
    let module = ref_module();
    let mut lock = Lockfile::new();
    let out = compile(&base_with_gui_ref(), &module, &mut lock, &opts()).unwrap();
    let anzeige = uuid(&lock, "anzeige");
    let spiegel = rect(&out, &gui_ref(&out, &lock));
    // Wired through the GUI's ref (D34), but nowhere near it.
    let moved = gui_move(&out, &anzeige, 19200, 19200);

    let laid = compile(&moved, &module, &mut lock, &relayout()).unwrap();
    // Up beside the ref, clear of the GUI's Memory that sits level with it.
    let a = rect(&laid, &anzeige);
    assert!(
        a.px > spiegel.px2 && a.px - spiegel.px2 < 4000,
        "{spiegel:?} {a:?}"
    );
    assert!((a.py - spiegel.py).abs() < 2 * GAP, "{spiegel:?} {a:?}");
    let geo = PageGeometry::of(&laid, BESCHATTUNG).unwrap();
    assert!(
        metrics(&geo).overlaps.is_empty(),
        "{:?}",
        metrics(&geo).overlaps
    );
}

#[test]
fn placement_is_per_page() {
    let one = "page \"Beschattung\"\n\
               extern sonne = VirtualIn(iname: \"VI3\")\n\
               fern = And(I1: sonne.Q)\n";
    let mut lock = Lockfile::new();
    let out = compile(
        &two_pages(),
        &Module::parse(one).unwrap(),
        &mut lock,
        &opts(),
    )
    .unwrap();
    // Far down on its own page.
    let moved = gui_move(&out, &uuid(&lock, "fern"), 960, 96000);

    // A new block on another page starts at that page's top, not below
    // the lowest block of the whole lock.
    let two = format!("{one}\npage \"Werkstatt\"\nnah = And(I1: sonne.Q)\n");
    let out = compile(&moved, &Module::parse(&two).unwrap(), &mut lock, &opts()).unwrap();
    let nah = rect(&out, &uuid(&lock, "nah"));
    assert!(nah.py < 4800, "{nah:?}");
}

/// `doc` with the object's extents redrawn, the way the GUI recomputes
/// `Px2`/`Py2` on every save without anyone dragging.
fn gui_redraw(doc: &LoxoneDoc, uuid: &str, dw: i64, dh: i64) -> LoxoneDoc {
    let mut doc = doc.clone();
    let r = rect(&doc, uuid);
    let path = doc.index().by_uuid[uuid].clone();
    let el = doc.element_at_mut(&path).unwrap();
    el.set_attr("Px2", &(r.px2 + dw).to_string());
    el.set_attr("Py2", &(r.py2 + dh).to_string());
    LoxoneDoc::parse(&doc.to_bytes()).unwrap()
}

fn two_pages() -> LoxoneDoc {
    let xml = std::fs::read_to_string("examples/configs/haus.Loxone").unwrap();
    LoxoneDoc::parse(
        xml.replace(
            PAGE_BESCHATTUNG,
            &format!("{PAGE_WERKSTATT}{PAGE_BESCHATTUNG}"),
        )
        .as_bytes(),
    )
    .unwrap()
}

#[test]
fn a_gui_save_that_only_redraws_extents_is_taken_over() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let and = uuid(&lock, "beschatten");
    let stale = gui_move(&out, &and, 28800, 19200);
    let laid = compile(&stale, &module(), &mut lock, &relayout()).unwrap();
    let fresh = rect(&laid, &and);

    // Downloaded after the push and saved: same spot, the GUI's extents.
    let saved = gui_redraw(&laid, &and, 0, ROW);
    let out = compile(&saved, &module(), &mut lock, &opts()).unwrap();
    let at = rect(&out, &and);
    assert!(at.same_spot(&fresh), "{fresh:?} {at:?}");
    assert_eq!(at, rect(&saved, &and));
    assert_eq!(lock.objects["beschatten"].layout, Some(at));
    assert_eq!(lock.objects["beschatten"].superseded_layout, None);
    let again = compile(&out, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(out.to_bytes(), again.to_bytes());
}

#[test]
fn a_stale_base_redrawn_by_the_gui_keeps_the_relayout() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let and = uuid(&lock, "beschatten");
    let stale = gui_move(&out, &and, 28800, 19200);
    let laid = compile(&stale, &module(), &mut lock, &relayout()).unwrap();
    let fresh = rect(&laid, &and);

    // The stale base, opened and saved once more before the download:
    // still at the replaced spot, only the extents recomputed.
    let resaved = gui_redraw(&stale, &and, 0, ROW);
    let before = lock.objects.clone();
    let out = compile(&resaved, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(rect(&out, &and), fresh);
    assert_eq!(lock.objects, before);
}

#[test]
fn a_relayout_of_a_laid_out_page_changes_nothing() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let stale = gui_move(&out, &uuid(&lock, "beschatten"), 28800, 19200);
    let laid = compile(&stale, &module(), &mut lock, &relayout()).unwrap();
    let again = compile(&laid, &module(), &mut lock, &relayout()).unwrap();
    assert_eq!(laid.to_bytes(), again.to_bytes());
    assert!(
        lock.objects.values().all(|e| e.superseded_layout.is_none()),
        "the base carries every placement"
    );
}

#[test]
fn an_unknown_relayout_page_is_an_error() {
    let opts = CompileOptions {
        relayout: vec!["Dachboden".into()],
        ..opts()
    };
    let err = compile(&base(), &module(), &mut Lockfile::new(), &opts).unwrap_err();
    assert!(err.to_string().contains("`Dachboden`"), "{err}");
}

#[test]
fn a_block_moved_to_another_page_is_placed_afresh() {
    let one = "page \"Beschattung\"\n\
               extern sonne = VirtualIn(iname: \"VI3\")\n\
               fern = And(I1: sonne.Q)\n";
    let mut lock = Lockfile::new();
    let out = compile(
        &two_pages(),
        &Module::parse(one).unwrap(),
        &mut lock,
        &opts(),
    )
    .unwrap();
    let fern = uuid(&lock, "fern");
    let moved = gui_move(&out, &fern, 960, 96000);

    // The module moves it: its old page's position means nothing on the
    // new one.
    let other = one.replace("page \"Beschattung\"", "page \"Werkstatt\"");
    let out = compile(&moved, &Module::parse(&other).unwrap(), &mut lock, &opts()).unwrap();
    let at = rect(&out, &fern);
    assert!(at.py < 4800, "{at:?}");
    assert_eq!(lock.objects["fern"].layout, Some(at));
    assert_eq!(lock.objects["fern"].superseded_layout, None);
}

#[test]
fn a_gui_paste_onto_another_page_is_not_a_move() {
    let one = "page \"Beschattung\"\n\
               extern sonne = VirtualIn(iname: \"VI3\")\n\
               fern = And(I1: sonne.Q)\n";
    let module = Module::parse(one).unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&two_pages(), &module, &mut lock, &opts()).unwrap();
    let fern = uuid(&lock, "fern");
    let placed = rect(&out, &fern);

    // Cut in the GUI and pasted far down the other page.
    let mut pasted = gui_move(&out, &fern, 960, 96000);
    let el = pasted.remove_by_uuid(&fern).unwrap();
    let werkstatt = pasted.page_path(Some("Werkstatt")).unwrap();
    pasted.element_at_mut(&werkstatt).unwrap().push_child(el);
    let pasted = LoxoneDoc::parse(&pasted.to_bytes()).unwrap();

    // The module still says Beschattung: rebuilt there, where it was.
    let before = lock.objects.clone();
    let out = compile(&pasted, &module, &mut lock, &opts()).unwrap();
    assert_eq!(rect(&out, &fern), placed);
    assert_eq!(lock.objects, before);
}

#[test]
fn a_memory_flag_is_placed_in_its_chain() {
    let module = Module::parse(
        "page \"Beschattung\"\n\
         extern sonne = VirtualIn(iname: \"VI3\")\n\
         \n\
         vorne = And(\n\
         \tI1: sonne.Q,\n\
         )\n\
         \n\
         merker = Memory(\n\
         \tInput: vorne.Q,\n\
         )\n\
         \n\
         hinten = And(\n\
         \tI1: merker.Q,\n\
         )\n",
    )
    .unwrap();
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module, &mut lock, &opts()).unwrap();
    let [v, m, h] = ["vorne", "merker", "hinten"].map(|s| rect(&out, &uuid(&lock, s)));
    assert_eq!((m.w(), m.h()), (TAG_W, ROW));
    assert!(v.px2 < m.px && m.px2 < h.px, "{v:?} {m:?} {h:?}");
}

/// `doc` drawn on A3 landscape, as most configs are (D50).
fn on_a3(doc: &LoxoneDoc) -> LoxoneDoc {
    let xml = String::from_utf8(doc.to_bytes()).unwrap();
    let doc_tag = "ConfigVersion=\"17010727\">";
    assert_eq!(xml.matches(doc_tag).count(), 1);
    LoxoneDoc::parse(
        xml.replace(doc_tag, "ConfigVersion=\"17010727\" PaperFormat=\"2\">")
            .as_bytes(),
    )
    .unwrap()
}

#[test]
fn the_sheet_comes_from_the_paper_format() {
    assert_eq!(lxir::layout::sheet(&base()), None);
    assert_eq!(
        lxir::layout::sheet(&on_a3(&base())),
        Some(Layout::at(0, 0, 23808, 16032))
    );
}

#[test]
fn a_relayout_brings_blocks_back_onto_the_sheet() {
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let and = uuid(&lock, "beschatten");
    let managed: std::collections::BTreeSet<&str> =
        lock.objects.values().map(|o| o.uuid.as_str()).collect();
    assert!(lxir::layout::off_sheet(&on_a3(&out), &managed).is_empty());

    // Dragged past the sheet's right edge in the GUI.
    let dragged = on_a3(&gui_move(&out, &and, 28800, 4800));
    let off = lxir::layout::off_sheet(&dragged, &managed);
    assert_eq!(off.get("Beschattung"), Some(&vec![and.clone()]));
    // A plain compile takes the drag over; a relayout brings it back.
    let kept = compile(&dragged, &module(), &mut lock, &opts()).unwrap();
    assert_eq!(rect(&kept, &and).px, 28800);
    let laid = compile(&dragged, &module(), &mut lock, &relayout()).unwrap();
    let managed: std::collections::BTreeSet<&str> =
        lock.objects.values().map(|o| o.uuid.as_str()).collect();
    assert!(
        lxir::layout::off_sheet(&laid, &managed).is_empty(),
        "{:?}",
        lxir::layout::off_sheet(&laid, &managed)
    );
    let geo = PageGeometry::of(&laid, BESCHATTUNG).unwrap();
    assert!(metrics(&geo).overlaps.is_empty());
}

/// The example base with the jalousie block dragged right, past the
/// sheet's edge: the chain feeding it is pulled after it.
fn jalousie_past_the_edge() -> LoxoneDoc {
    let base = base();
    let geo = PageGeometry::of(&base, BESCHATTUNG).unwrap();
    let jal = geo
        .objects
        .iter()
        .find(|o| o.title == "Beschattung Süd")
        .unwrap();
    gui_move(&base, &jal.uuid, 26880, jal.rect.py)
}

#[test]
fn placement_keeps_to_the_sheet_where_the_wires_pull_past_it() {
    let sheet = Layout::at(0, 0, 23808, 16032);
    // Unbounded, the chain sits beside the jalousie, past the edge.
    let mut lock = Lockfile::new();
    let open = compile(&jalousie_past_the_edge(), &module(), &mut lock, &opts()).unwrap();
    let and = rect(&open, &uuid(&lock, "beschatten"));
    assert!(!and.within(&sheet), "{and:?}");

    // On A3 it stays on the sheet, and a relayout changes nothing.
    let mut lock = Lockfile::new();
    let base = on_a3(&jalousie_past_the_edge());
    let out = compile(&base, &module(), &mut lock, &opts()).unwrap();
    let managed: std::collections::BTreeSet<&str> =
        lock.objects.values().map(|o| o.uuid.as_str()).collect();
    assert!(
        lxir::layout::off_sheet(&out, &managed).is_empty(),
        "{:?}",
        lxir::layout::off_sheet(&out, &managed)
    );
    let and = rect(&out, &uuid(&lock, "beschatten"));
    assert!(and.within(&sheet), "{and:?}");
    let laid = compile(&out, &module(), &mut lock, &relayout()).unwrap();
    assert_eq!(out.to_bytes(), laid.to_bytes());
}

#[test]
fn off_sheet_counts_labels_like_render() {
    // "Temp über 28" reads a value from off the page: its label sits left
    // of it, so at x 0 the label hangs past the sheet's edge.
    let mut lock = Lockfile::new();
    let out = compile(&base(), &module(), &mut lock, &opts()).unwrap();
    let temp = uuid(&lock, "temp_hoch");
    let moved = on_a3(&gui_move(&out, &temp, 0, rect(&out, &temp).py));
    let managed: std::collections::BTreeSet<&str> =
        lock.objects.values().map(|o| o.uuid.as_str()).collect();
    assert_eq!(
        lxir::layout::off_sheet(&moved, &managed).get("Beschattung"),
        Some(&vec![temp.clone()])
    );
    let geo = PageGeometry::of(&moved, BESCHATTUNG).unwrap();
    assert!(
        metrics(&geo)
            .off_sheet
            .contains(&"Temp über 28".to_string())
    );
}
