#!/usr/bin/env python3
"""Generate src/autopilot_caps.rs from the Automatic Designer capability
file the Miniserver serves to the app:

    curl -u USER:PASS http://MINISERVER/jdev/sps/io/<AutoPilot uuid>/capabilities/getraw > caps.json
    python3 scripts/autopilot_caps.py caps.json > src/autopilot_caps.rs

Only the codes are written, not the localized texts."""
import json,sys
d=json.load(open(sys.argv[1]))
out=[]
w=out.append
w("//! The Automatic Designer's codes for conditions and actions on blocks,")
w("//! as the Miniserver serves them to the app (`jdev/sps/io/<AutoPilot")
w("//! container>/capabilities/getraw`, version %s, read from the" % d['version'])
w("//! house's Miniserver, firmware 17.2, 2026-10-31). The app builds every")
w("//! rule from this table: a condition on a block carries the `descID` of")
w("//! one of its `events`, the block's `uuidAction` as `controlUUID` and the")
w("//! UUID of the state named by `sourceState` as `id`; an action carries the")
w("//! `descID` of one of its `actions` and the command text of `action`, its")
w("//! `<valN>` placeholders filled in. Only the codes are kept, not the")
w("//! localized texts. Generated; one row per app control type.")
w("")
w("/// What the value of a condition or an action is.")
w("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
w("pub enum Kind {")
w("    Digital,")
w("    Analog,")
w("    Text,")
w("    Pulse,")
w("}")
w("")
w("/// A condition a block offers: its code, the state it watches and, for a")
w("/// digital one, the value it waits for.")
w("#[derive(Debug, Clone, Copy)]")
w("pub struct EventCap {")
w("    pub desc: i64,")
w("    pub kind: Kind,")
w("    pub state: Option<&'static str>,")
w("    pub value: Option<i64>,")
w("}")
w("")
w("/// An action a block offers: its code and its command text.")
w("#[derive(Debug, Clone, Copy)]")
w("pub struct ActionCap {")
w("    pub desc: i64,")
w("    pub kind: Kind,")
w("    pub command: &'static str,")
w("}")
w("")
w("/// The conditions and actions of the blocks of some app control types.")
w("#[derive(Debug, Clone, Copy)]")
w("pub struct Capability {")
w("    pub control_types: &'static [&'static str],")
w("    pub events: &'static [EventCap],")
w("    pub actions: &'static [ActionCap],")
w("}")
w("")
K={'digital':'Digital','analog':'Analog','text':'Text','pulse':'Pulse'}
def opt(v,f): return 'None' if v is None else 'Some(%s)'%f(v)
w("pub const CAPABILITIES: &[Capability] = &[")
skipped=[]
for e in d['entries']:
    if e['type']!=0: continue
    w("    Capability {")
    w("        control_types: &[%s]," % ", ".join(json.dumps(t) for t in e['controlType']))
    evs=[]
    for x in e.get('events',[]):
        sv=x.get('sourceValue')
        if isinstance(sv,list) or x['type'] not in K: skipped.append(x['descID']); continue
        evs.append("EventCap { desc: %d, kind: Kind::%s, state: %s, value: %s }"%(x['descID'],K[x['type']],opt(x.get('sourceState'),json.dumps),opt(sv,str)))
    acs=[]
    for x in e.get('actions',[]):
        if x['type'] not in K or 'action' not in x: skipped.append(x['descID']); continue
        acs.append("ActionCap { desc: %d, kind: Kind::%s, command: %s }"%(x['descID'],K[x['type']],json.dumps(x['action'])))
    for name,items in (('events',evs),('actions',acs)):
        if not items: w("        %s: &[],"%name); continue
        w("        %s: &["%name)
        for i in items: w("            %s,"%i)
        w("        ],")
    w("    },")
w("];")
sys.stderr.write("skipped %s\n"%skipped)
print("\n".join(out))
