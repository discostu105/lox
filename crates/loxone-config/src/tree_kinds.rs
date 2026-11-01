//! The Tree devices a declaration can create: each kind's element as a
//! Loxone Config save writes it, with its inputs and outputs. A kind is
//! listed only when the public configs show it with one set of I/O and a
//! save keeps a created one unchanged (see docs/connector-db.md). Titles
//! are the English defaults; the device is unpaired (`Serial="00000000"`).

use crate::uuid::Minter;
use crate::xml::{Element, Node, XmlDocument};

/// A Tree device kind.
pub struct TreeKind {
    /// Loxone Config's name for it, its `TId=`.
    pub name: &'static str,
    /// The title Loxone Config gives a new one.
    pub title: &'static str,
    /// The element: `@T` is the title, `@V` the version of its Tree
    /// extension, `@U` an object or connector UUID.
    template: &'static str,
}

const KINDS: &[TreeKind] = &[
    TreeKind {
        name: "TouchTree",
        title: "Touch Tree",
        template: r#"<C Type="TreeDevice" TId="TouchTree" V="@V" U="@U" Title="@T" WF="16384" Serial="00000000" Installation="" SubType="32771" TreeType="32771" PG="">
	<IoData></IoData>
	<C Type="Online" IName="S0" V="@V" U="@U" Title="Online Status Touch Tree" Cl="0,0,0" Nio="1" WF="20480" ValOT="1" MinTime="10" Idx="-1">
		<Co K="Q" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true" IxText="8"/>
	</C>
	<C Type="TreeSensor" IName="I1" V="@V" U="@U" Title="I1" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="16" Idx="1">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I2" V="@V" U="@U" Title="I2" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="17" Idx="2">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I3" V="@V" U="@U" Title="I3" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="18" Idx="3">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I4" V="@V" U="@U" Title="I4" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="19" Idx="4">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I5" V="@V" U="@U" Title="I5" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="20" Idx="5">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeAsensor" IName="AI1" V="@V" U="@U" Title="Temperature" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" Analog="true" EnoceanSubType="1" SourceValHigh="100" DestValHigh="100" MinVal="-40" MaxVal="125" MinChange="0.25" MinTime="1000" Idx="1">
		<Co K="AQ" U="@U"/>
		<Co K="Q" U="@U"/>
		<IoData St="4"/>
		<Display Type="2" Unit="&lt;v.1&gt;°" StateOnly="true"/>
	</C>
	<C Type="TreeAsensor" IName="AI2" V="@V" U="@U" Title="Humidity" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" Analog="true" EnoceanSubType="2" SourceValHigh="1" DestValHigh="1" MaxVal="100" MinChange="0.25" MinTime="1000" Idx="2">
		<Co K="AQ" U="@U"/>
		<Co K="Q" U="@U"/>
		<IoData St="4"/>
		<Display Type="2" Unit="&lt;v&gt;%" StateOnly="true"/>
	</C>
	<C Type="TreeAsensor" IName="AI3" V="@V" U="@U" Title="T5" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" Analog="true" SourceValHigh="10" DestValHigh="10" MaxVal="10" MinChange="0.25" MinTime="1000" Idx="3">
		<Co K="AQ" U="@U"/>
		<Co K="Q" U="@U"/>
		<IoData St="4"/>
		<Display Type="2" Unit="&lt;v.i&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeActor" IName="Q1" V="@V" U="@U" Title="Audible click signal" Cl="0,0,0" Nio="1" WF="20480" SubType="163" Idx="1">
		<Co K="I" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v.1&gt;" StateOnly="true"/>
	</C>
</C>"#,
    },
    TreeKind {
        name: "PresenceSensorTree",
        title: "Presence Sensor Tree",
        template: r#"<C Type="TreeDevice" TId="PresenceSensorTree" V="@V" U="@U" Title="@T" WF="16384" Serial="00000000" Installation="" SubType="32794" TreeType="32794" PG="" SendC="300" SendCB="900" OnTime="900" OnTimeM="60" TrPres="250" TrA="1000" LS="1" OnTimeLim="1800">
	<IoData></IoData>
	<C Type="Online" IName="S0" V="@V" U="@U" Title="Online Status Presence Sensor Tree" Cl="0,0,0" Nio="1" WF="20480" ValOT="1" MinTime="10" Idx="-1">
		<Co K="Q" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true" IxText="8"/>
	</C>
	<C Type="TreeSensor" IName="I1" V="@V" U="@U" Title="Motion" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" MinTime="10" EnoceanSubType="4" Idx="1">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I2" V="@V" U="@U" Title="Presence" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" MinTime="10" EnoceanSubType="170" Idx="2">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I3" V="@V" U="@U" Title="Sound Level Alarm" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" MinTime="10" EnoceanSubType="168" Idx="3">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeAsensor" IName="AI1" V="@V" U="@U" Title="Volume Minimum" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" Analog="true" EnoceanSubType="57" SourceValHigh="10" DestValHigh="10" MaxVal="10" MinChange="0.25" MinTime="1000" Idx="1">
		<Co K="AQ" U="@U"/>
		<Co K="Q" U="@U"/>
		<IoData St="4"/>
		<Display Type="2" Unit="&lt;v.1&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeAsensor" IName="AI2" V="@V" U="@U" Title="Volume Maximum" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" Analog="true" EnoceanSubType="58" SourceValHigh="10" DestValHigh="10" MaxVal="10" MinChange="0.25" MinTime="1000" Idx="2">
		<Co K="AQ" U="@U"/>
		<Co K="Q" U="@U"/>
		<IoData St="4"/>
		<Display Type="2" Unit="&lt;v.1&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeAsensor" IName="AI3" V="@V" U="@U" Title="Brightness" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" Analog="true" EnoceanSubType="5" SourceValHigh="1" DestValHigh="1" MaxVal="10000" MinChange="0.25" MinTime="1000" Idx="3">
		<Co K="AQ" U="@U"/>
		<Co K="Q" U="@U"/>
		<IoData St="4"/>
		<Display Type="2" Unit="&lt;v&gt;Lx" StateOnly="true"/>
	</C>
	<C Type="TreeAactor" IName="AQ1" V="@V" U="@U" Title="Overrun Time Presence" Cl="0,0,0" Nio="1" WF="20480" Percent="true" Channel="1" SourceValHigh="100" DestValHigh="100" DimmType="3" SubType="61" Idx="1">
		<Co K="I" U="@U"/>
		<IoData></IoData>
		<Display Unit="&lt;v&gt;%" StateOnly="true"/>
	</C>
	<C Type="TreeAactor" IName="AQ2" V="@V" U="@U" Title="Time" Cl="0,0,0" Nio="1" WF="20480" Percent="true" Channel="1" SourceValHigh="100" DestValHigh="100" DimmType="3" SubType="62" Idx="2">
		<Co K="I" U="@U"/>
		<IoData></IoData>
		<Display Unit="&lt;v&gt;%" StateOnly="true"/>
	</C>
</C>"#,
    },
    TreeKind {
        name: "CorridorLightTree",
        title: "LED Ceiling Light RGBW Tree",
        template: r#"<C Type="TreeDevice" TId="CorridorLightTree" V="@V" U="@U" Title="@T" WF="16384" Serial="00000000" Installation="" SubType="32774" TreeType="32774" PG="" LI="1" OT="60">
	<IoData></IoData>
	<C Type="Online" IName="S0" V="@V" U="@U" Title="Online Status LED Ceiling Light RGBW Tree" Cl="0,0,0" Nio="1" WF="20480" ValOT="1" MinTime="10" Idx="-1">
		<Co K="Q" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true" IxText="8"/>
	</C>
	<C Type="TreeSensor" IName="I1" V="@V" U="@U" Title="Motion" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" MinTime="10" EnoceanSubType="4" Idx="1">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeAsensor" IName="AI1" V="@V" U="@U" Title="Brightness" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" Analog="true" EnoceanSubType="5" SourceValHigh="1" DestValHigh="1" MaxVal="10000" MinChange="0.25" MinTime="1000" Idx="1">
		<Co K="AQ" U="@U"/>
		<Co K="Q" U="@U"/>
		<IoData St="4"/>
		<Display Type="2" Unit="&lt;v&gt;Lx" StateOnly="true"/>
	</C>
	<C Type="TreeAactor" IName="AQ1" V="@V" U="@U" Title="Smart Actuator RGBW" Cl="0,0,0" Nio="1" WF="16384" Percent="true" Channel="1" SourceValHigh="100" DestValHigh="100" DimmType="3" CCap="31" SubType="158" Idx="1">
		<Co K="I" U="@U"/>
		<IoData></IoData>
		<Display Unit="&lt;v.col&gt;" StateOnly="true"/>
	</C>
</C>"#,
    },
    TreeKind {
        name: "ValveTree",
        title: "Valve Actuator Tree",
        template: r#"<C Type="TreeDevice" TId="ValveTree" V="@V" U="@U" Title="@T" WF="16384" Serial="00000000" Installation="" SwitchBoard="" SubType="32769" TreeType="32769" PG="" SwitchBoardPos="1">
	<IoData></IoData>
	<C Type="Online" IName="S0" V="@V" U="@U" Title="Online Status Valve Actuator Tree" Cl="0,0,0" Nio="1" WF="20480" ValOT="1" MinTime="10" Idx="-1">
		<Co K="Q" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true" IxText="8"/>
	</C>
	<C Type="TreeAactor" IName="AQ1" V="@V" U="@U" Title="Valve Actuator Tree" Cl="0,0,0" Nio="1" WF="16384" Percent="true" Channel="1" SourceValHigh="10" DestValHigh="100" DimmType="3" SubType="24" Idx="1">
		<Co K="I" U="@U"/>
		<IoData></IoData>
		<Display Unit="&lt;v&gt;%" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I1" V="@V" U="@U" Title="No Valve Detected" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="77" Idx="1">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true" IxText="9" IxColor="1"/>
	</C>
	<C Type="TreeSensor" IName="I2" V="@V" U="@U" Title="Valve is stuck" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="78" Idx="2">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true" IxText="10" IxColor="1"/>
	</C>
	<C Type="TreeSensor" IName="I3" V="@V" U="@U" Title="Hardware defect" Cl="0,0,0" Nio="2" WF="20480" ValOT="1" MinTime="10" EnoceanSubType="166" Idx="3">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
	<C Type="TreeSensor" IName="I4" V="@V" U="@U" Title="Input 1" Cl="0,0,0" Nio="2" WF="16384" ValOT="1" MinTime="10" EnoceanSubType="16" Idx="4">
		<Co K="Q" U="@U"/>
		<Co K="Qe" U="@U"/>
		<IoData St="2"/>
		<Display Unit="&lt;v&gt;" StateOnly="true"/>
	</C>
</C>"#,
    },
];

/// The kind named `name`.
pub fn kind(name: &str) -> Option<&'static TreeKind> {
    KINDS.iter().find(|k| k.name == name)
}

/// The kind names, for messages.
pub fn names() -> Vec<&'static str> {
    KINDS.iter().map(|k| k.name).collect()
}

impl TreeKind {
    /// The element with `title`, `version` and fresh UUIDs: objects from
    /// `minter`, connectors from it with `entity` (index counted per
    /// object).
    pub fn element(
        &self,
        title: &str,
        version: &str,
        minter: &mut Minter,
        entity: [u8; 6],
    ) -> Element {
        let text = self
            .template
            .replace("@T", &crate::xml::escape(title))
            .replace("@V", version);
        let doc = XmlDocument::parse(text.as_bytes()).expect("a kind's template parses");
        let mut el = doc.root;
        fill(&mut el, minter, entity);
        el
    }
}

fn fill(el: &mut Element, minter: &mut Minter, entity: [u8; 6]) {
    if el.attr("U") == Some("@U") {
        let u = minter.mint_object().to_string();
        el.set_attr("U", &u);
    }
    let mut n = 0u8;
    for child in &mut el.children {
        let Node::Element(c) = child else { continue };
        if c.name == "Co" {
            let u = minter.mint_port(n, entity).to_string();
            c.set_attr("U", &u);
            n += 1;
        } else {
            fill(c, minter, entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_parses_with_unique_uuids() {
        for k in KINDS {
            let mut minter = Minter::new([1, 2, 3, 4, 5, 6], 1_767_225_600);
            let el = k.element("X & Y", "178", &mut minter, [9; 6]);
            assert_eq!(el.attr("TId"), Some(k.name));
            assert_eq!(el.attr("Title"), Some("X &amp; Y"));
            let mut uuids = Vec::new();
            let mut stack = vec![&el];
            while let Some(e) = stack.pop() {
                uuids.extend(e.attr("U").map(str::to_string));
                stack.extend(e.child_elements());
            }
            let n = uuids.len();
            uuids.sort();
            uuids.dedup();
            assert_eq!(uuids.len(), n, "{}", k.name);
            assert!(uuids.iter().all(|u| !u.contains('@')), "{}", k.name);
        }
    }
}
