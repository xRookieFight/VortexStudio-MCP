//! The Vortex Luau API reference, loaded from `data/api.json` at compile time.

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const RAW: &str = include_str!("../data/api.json");

#[derive(Debug, Deserialize)]
pub struct Api {
    pub about: serde_json::Value,
    pub globals: Vec<Member>,
    pub libraries: BTreeMap<String, Vec<String>>,
    pub services: Vec<Type>,
    pub classes: Vec<Type>,
    pub datatypes: Vec<Type>,
    pub enums: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub enums_unverified: BTreeMap<String, Vec<String>>,
    pub limits: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Type {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub creatable: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub r#abstract: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unverified: bool,
    #[serde(default)]
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Member {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub readonly: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unverified: bool,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    pub kind: &'static str,
    pub owner: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn get() -> &'static Api {
    static API: OnceLock<Api> = OnceLock::new();
    API.get_or_init(|| {
        serde_json::from_str(RAW).expect("data/api.json is valid, a unit test checks it")
    })
}

impl Api {
    /// Every type with this name: a service, a class or a datatype (Instance is both).
    pub fn find_type(&self, name: &str) -> Vec<(&'static str, &Type)> {
        let mut out = Vec::new();
        for (kind, list) in [
            ("service", &self.services),
            ("class", &self.classes),
            ("datatype", &self.datatypes),
        ] {
            if let Some(t) = list.iter().find(|t| t.name.eq_ignore_ascii_case(name)) {
                out.push((kind, t));
            }
        }
        out
    }

    /// Members of a class including the ones it inherits, nearest class first.
    pub fn members_with_inherited<'a>(&'a self, t: &'a Type) -> Vec<(&'a str, &'a Member)> {
        let mut out: Vec<(&str, &Member)> =
            t.members.iter().map(|m| (t.name.as_str(), m)).collect();
        let mut parent = t.inherits.as_deref();
        let mut guard = 0;
        while let Some(name) = parent {
            let Some(p) = self.classes.iter().find(|c| c.name == name) else {
                break;
            };
            out.extend(p.members.iter().map(|m| (p.name.as_str(), m)));
            parent = p.inherits.as_deref();
            guard += 1;
            if guard > 16 {
                break;
            }
        }
        out
    }

    pub fn find_enum(&self, name: &str) -> Option<(&String, &Vec<String>)> {
        let name = name.trim_start_matches("Enum.");
        self.enums
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
    }

    /// Case insensitive search over names, signatures and descriptions. Name
    /// matches rank first, then everything else in document order.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q = query.to_lowercase();
        let mut exact = Vec::new();
        let mut name_hits = Vec::new();
        let mut text_hits = Vec::new();
        let mut push = |hit: Hit, name: &str, text: &str| {
            let n = name.to_lowercase();
            if n == q {
                exact.push(hit);
            } else if n.contains(&q) {
                name_hits.push(hit);
            } else if text.to_lowercase().contains(&q) {
                text_hits.push(hit);
            }
        };

        for (kind, list) in [
            ("service", &self.services),
            ("class", &self.classes),
            ("datatype", &self.datatypes),
        ] {
            for t in list {
                push(
                    Hit {
                        kind,
                        owner: None,
                        name: t.name.clone(),
                        detail: t.description.clone(),
                    },
                    &t.name,
                    t.description.as_deref().unwrap_or(""),
                );
                for m in &t.members {
                    let text = format!(
                        "{} {}",
                        m.signature.as_deref().unwrap_or(""),
                        m.description.as_deref().unwrap_or("")
                    );
                    push(
                        Hit {
                            kind: member_kind(&m.kind),
                            owner: Some(t.name.clone()),
                            name: m.name.clone(),
                            detail: m.signature.clone().or_else(|| m.r#type.clone()),
                        },
                        &m.name,
                        &text,
                    );
                }
            }
        }
        for g in &self.globals {
            push(
                Hit {
                    kind: "global",
                    owner: None,
                    name: g.name.clone(),
                    detail: g.signature.clone(),
                },
                &g.name,
                g.description.as_deref().unwrap_or(""),
            );
        }
        for (lib, funcs) in &self.libraries {
            for f in funcs {
                let full = format!("{lib}.{f}");
                push(
                    Hit {
                        kind: "library",
                        owner: Some(lib.clone()),
                        name: f.clone(),
                        detail: None,
                    },
                    &full,
                    "",
                );
            }
        }
        for (e, items) in &self.enums {
            push(
                Hit {
                    kind: "enum",
                    owner: None,
                    name: format!("Enum.{e}"),
                    detail: Some(items.join(", ")),
                },
                e,
                &items.join(" "),
            );
        }

        exact.extend(name_hits);
        exact.extend(text_hits);
        exact.truncate(limit);
        exact
    }

    pub fn service_names(&self) -> HashSet<&str> {
        self.services.iter().map(|s| s.name.as_str()).collect()
    }

    pub fn creatable_classes(&self) -> HashSet<&str> {
        self.classes
            .iter()
            .filter(|c| c.creatable)
            .map(|c| c.name.as_str())
            .collect()
    }

    pub fn global_names(&self) -> HashSet<&str> {
        let mut names: HashSet<&str> = self.globals.iter().map(|g| g.name.as_str()).collect();
        names.extend(self.libraries.keys().map(String::as_str));
        names.extend(self.datatypes.iter().map(|d| d.name.as_str()));
        names
    }
}

fn member_kind(kind: &str) -> &'static str {
    match kind {
        "property" => "property",
        "method" => "method",
        "event" => "event",
        "callback" => "callback",
        "constructor" => "constructor",
        "constant" => "constant",
        _ => "member",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_json_loads_and_is_consistent() {
        let api = get();
        assert!(api.services.iter().any(|s| s.name == "TweenService"));
        // every inherits must point at a real class
        for c in &api.classes {
            if let Some(p) = &c.inherits {
                assert!(
                    api.classes.iter().any(|x| &x.name == p),
                    "{} inherits unknown {p}",
                    c.name
                );
            }
        }
        // every class the file format knows should be documented
        for class in crate::vrtx::Class::ALL {
            let name = class.to_string();
            assert!(
                !api.find_type(&name).is_empty(),
                "{name} is in the file format but not in api.json"
            );
        }
    }

    #[test]
    fn part_inherits_instance_members() {
        let api = get();
        let part = api.find_type("Part")[0].1;
        let members = api.members_with_inherited(part);
        assert!(members.iter().any(|(_, m)| m.name == "Anchored"));
        assert!(
            members
                .iter()
                .any(|(owner, m)| *owner == "Instance" && m.name == "Destroy")
        );
    }

    #[test]
    fn search_ranks_exact_names_first() {
        let hits = get().search("Raycast", 5);
        assert_eq!(hits[0].name, "Raycast");
        assert!(
            get()
                .search("fromRGB", 3)
                .iter()
                .any(|h| h.owner.as_deref() == Some("Color3"))
        );
        assert!(
            get().search("Enum.Material", 3).is_empty()
                || get().find_enum("Enum.Material").is_some()
        );
    }
}
