//! Reading the Miniserver's XML answers with quick-xml: the element name and
//! its attributes, in one place for every parser.

use quick_xml::XmlVersion;
use quick_xml::events::BytesStart;

/// The element's tag name.
pub fn tag(e: &BytesStart) -> String {
    e.name().as_ref().to_string()
}

/// The attribute `name` as written, entities unresolved.
pub fn raw_attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| a.value.to_string())
}

/// The attribute `name` as XML defines its value: entities resolved (`&amp;`
/// → `&`), line breaks and tabs as spaces. `None` when it is missing or an
/// entity is unknown.
pub fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| {
            a.normalized_value(XmlVersion::default())
                .ok()
                .map(|v| v.to_string())
        })
}

/// Every attribute as (name, value), entities resolved where they can be.
pub fn attrs(e: &BytesStart) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .map(|a| {
            let value = a
                .normalized_value(XmlVersion::default())
                .map(|v| v.to_string())
                .unwrap_or_else(|_| a.value.to_string());
            (a.key.as_ref().to_string(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::Reader;
    use quick_xml::events::Event;

    fn first(xml: &str, read: impl Fn(&BytesStart)) {
        let mut reader = Reader::from_str(xml);
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => return read(&e),
                Ok(Event::Eof) | Err(_) => panic!("no element in {xml}"),
                _ => {}
            }
        }
    }

    #[test]
    fn reads_the_tag_and_attributes_with_umlauts_and_entities() {
        let xml =
            r#"<?xml version="1.0"?><TreeDevice Name="Küche &amp; Bad" Place=" EG " Empty=""/>"#;
        first(xml, |e| {
            assert_eq!(tag(e), "TreeDevice");
            assert_eq!(raw_attr(e, "Name").as_deref(), Some("Küche &amp; Bad"));
            assert_eq!(attr(e, "Name").as_deref(), Some("Küche & Bad"));
            assert_eq!(raw_attr(e, "Place").as_deref(), Some(" EG "));
            assert_eq!(raw_attr(e, "Empty").as_deref(), Some(""));
            assert_eq!(raw_attr(e, "Missing"), None);
            assert_eq!(raw_attr(e, "name"), None, "names are case-sensitive");
            assert_eq!(
                attrs(e),
                [
                    ("Name".to_string(), "Küche & Bad".to_string()),
                    ("Place".to_string(), " EG ".to_string()),
                    ("Empty".to_string(), String::new()),
                ]
            );
        });
    }

    #[test]
    fn an_unknown_entity_keeps_the_raw_value_in_attrs() {
        first(r#"<output name="a &unknown; b"/>"#, |e| {
            assert_eq!(attr(e, "name"), None);
            assert_eq!(
                attrs(e),
                [("name".to_string(), "a &unknown; b".to_string())]
            );
        });
    }
}
