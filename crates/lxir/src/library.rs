//! The template library (`templates/`): ready-made `.lxir` templates for
//! common automations, each tested in the simulator (`templates/tests/`).
//! `lxir templates` lists them and prints a file to copy into a module.

use crate::ir::{Item, Module};

/// The library files: name and source.
pub const FILES: &[(&str, &str)] = &[
    ("licht", include_str!("../templates/licht.lxir")),
    ("klima", include_str!("../templates/klima.lxir")),
    ("sicherheit", include_str!("../templates/sicherheit.lxir")),
    ("beschattung", include_str!("../templates/beschattung.lxir")),
];

/// One template of the library.
pub struct Entry {
    pub file: &'static str,
    pub name: String,
    /// The parameter list as written: `bewegung: any, nachlauf = 3min`.
    pub params: String,
    /// The comment opening the body, joined into one line.
    pub description: String,
}

/// Every template, in file order.
pub fn entries() -> Vec<Entry> {
    let mut out = Vec::new();
    for (file, source) in FILES {
        let module = Module::parse_fragment(source).expect("the library parses");
        for item in module.items {
            let Item::Template(t) = item else { continue };
            let description = t
                .body
                .iter()
                .map_while(|i| match i {
                    Item::Comment(c) => Some(c.trim()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.push(Entry {
                file,
                name: t.name,
                params: t
                    .params
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                description,
            });
        }
    }
    out
}

/// The source of a library file, by its name or the name of a template in it.
pub fn source(name: &str) -> Option<(&'static str, &'static str)> {
    if let Some(&(file, src)) = FILES.iter().find(|(f, _)| *f == name) {
        return Some((file, src));
    }
    let file = entries().into_iter().find(|e| e.name == name)?.file;
    FILES.iter().find(|(f, _)| *f == file).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_has_a_description_and_a_unique_name() {
        let entries = entries();
        assert!(entries.len() >= 10);
        let mut names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), entries.len());
        for e in &entries {
            assert!(!e.description.is_empty(), "{}", e.name);
        }
        assert_eq!(source("treppenlicht").unwrap().0, "licht");
        assert_eq!(source("klima").unwrap().0, "klima");
        assert!(source("nichts").is_none());
    }
}
