//! Whole project health check: structure, placement and every script's lint.

use std::collections::HashMap;

use serde::Serialize;

use crate::lint::{self, ScriptKind, Severity};
use crate::scene;
use crate::vrtx::{Class, Project};

const SCRIPT_LIMIT: usize = 256;

#[derive(Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub rule: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub message: String,
}

pub fn script_kind(class: Class) -> Option<ScriptKind> {
    match class {
        Class::Script => Some(ScriptKind::Script),
        Class::LocalScript => Some(ScriptKind::LocalScript),
        Class::ModuleScript => Some(ScriptKind::ModuleScript),
        _ => None,
    }
}

/// The service an instance lives under.
fn service_of(p: &Project, mut idx: usize) -> Option<Class> {
    for _ in 0..=p.instances.len() {
        let inst = p.instances.get(idx)?;
        match inst.parent {
            None => return Some(inst.class),
            Some(par) => idx = par as usize,
        }
    }
    None
}

pub fn check(p: &Project) -> Vec<Finding> {
    let mut out = Vec::new();
    let at = |i: usize| Some(format!("#{i} {}", scene::path_of(p, i)));

    // structure first, the rest assumes a sane tree
    for (i, inst) in p.instances.iter().enumerate() {
        if let Some(par) = inst.parent {
            let par = par as usize;
            if par >= p.instances.len() {
                out.push(Finding {
                    severity: Severity::Error,
                    rule: "broken-parent",
                    instance: Some(format!("#{i} {}", inst.name)),
                    line: None,
                    message: format!("parent #{par} doesn't exist"),
                });
            } else if par >= i {
                out.push(Finding {
                    severity: Severity::Warning,
                    rule: "parent-order",
                    instance: at(i),
                    line: None,
                    message: "listed before its parent, Studio always saves parents first".into(),
                });
            }
        } else if !inst.class.is_service() {
            out.push(Finding {
                severity: Severity::Warning,
                rule: "orphan",
                instance: Some(format!("#{i} {}", inst.name)),
                line: None,
                message: "has no parent and isn't a service".into(),
            });
        }
        if let Class::Unknown(t) = inst.class {
            out.push(Finding {
                severity: Severity::Warning,
                rule: "unknown-class",
                instance: at(i),
                line: None,
                message: format!("class id {t} is newer than this tool knows about"),
            });
        }
        if let Some(part) = &inst.part {
            if part.name != inst.name {
                out.push(Finding {
                    severity: Severity::Info,
                    rule: "name-mismatch",
                    instance: at(i),
                    line: None,
                    message: format!(
                        "part data is named {:?}, rename the instance to sync them",
                        part.name
                    ),
                });
            }
            if part.color[3] <= 0.0 && part.can_collide {
                out.push(Finding {
                    severity: Severity::Info,
                    rule: "invisible-wall",
                    instance: at(i),
                    line: None,
                    message: "fully transparent but still collides, players will bump into nothing"
                        .into(),
                });
            }
            if !part.anchored && part.baseplate {
                out.push(Finding {
                    severity: Severity::Warning,
                    rule: "loose-baseplate",
                    instance: at(i),
                    line: None,
                    message: "the baseplate isn't anchored and will fall".into(),
                });
            }
        }
    }

    // duplicate names make FindFirstChild and WaitForChild pick one arbitrarily
    let mut siblings: HashMap<(Option<u64>, &str), Vec<usize>> = HashMap::new();
    for (i, inst) in p.instances.iter().enumerate() {
        siblings
            .entry((inst.parent, inst.name.as_str()))
            .or_default()
            .push(i);
    }
    let mut dupes: Vec<_> = siblings.into_iter().filter(|(_, v)| v.len() > 1).collect();
    dupes.sort_by_key(|(_, v)| v[0]);
    for ((_, name), idxs) in dupes {
        let referenced = p
            .instances
            .iter()
            .filter_map(|i| i.script.as_ref())
            .any(|s| s.source.contains(&format!("\"{name}\"")));
        out.push(Finding {
            severity: if referenced {
                Severity::Warning
            } else {
                Severity::Info
            },
            rule: "duplicate-name",
            instance: at(idxs[0]),
            line: None,
            message: format!(
                "{} siblings are named {name:?} ({}){}",
                idxs.len(),
                idxs.iter()
                    .map(|i| format!("#{i}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                if referenced {
                    ", and a script looks it up by that name"
                } else {
                    ""
                }
            ),
        });
    }

    let mut scripts = 0;
    for (i, inst) in p.instances.iter().enumerate() {
        let (Some(kind), Some(script)) = (script_kind(inst.class), &inst.script) else {
            continue;
        };
        scripts += 1;
        let service = service_of(p, i);
        let misplaced = match kind {
            ScriptKind::Script => {
                !matches!(service, Some(Class::ServerScriptService | Class::Workspace))
            }
            ScriptKind::LocalScript => !matches!(service, Some(Class::StarterPlayerScripts)),
            ScriptKind::ModuleScript => false,
        };
        if misplaced && script.enabled {
            out.push(Finding {
                severity: Severity::Warning,
                rule: "script-placement",
                instance: at(i),
                line: None,
                message: match kind {
                    ScriptKind::Script => "server Scripts run from ServerScriptService or Workspace, this one likely never runs".into(),
                    _ => "LocalScripts run from StarterPlayerScripts, this one likely never runs".into(),
                },
            });
        }
        if !script.enabled {
            out.push(Finding {
                severity: Severity::Info,
                rule: "disabled-script",
                instance: at(i),
                line: None,
                message: "script is disabled".into(),
            });
        }
        for d in lint::lint(&script.source, kind) {
            out.push(Finding {
                severity: d.severity,
                rule: d.rule,
                instance: at(i),
                line: Some(d.line),
                message: d.message,
            });
        }
    }
    if scripts > SCRIPT_LIMIT {
        out.push(Finding {
            severity: Severity::Error,
            rule: "script-limit",
            instance: None,
            line: None,
            message: format!("{scripts} scripts, Vortex only starts the first {SCRIPT_LIMIT}"),
        });
    }

    out.sort_by_key(|f| std::cmp::Reverse(f.severity));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vrtx;

    #[test]
    fn showcase_findings() {
        let p = vrtx::decode(include_bytes!("../tests/fixtures/showcase.vrtx")).unwrap();
        let found = check(&p);
        let rules: Vec<_> = found.iter().map(|f| f.rule).collect();
        // the LocalScript sits in ReplicatedStorage and there are several parts called "Part"
        assert!(rules.contains(&"script-placement"), "{rules:?}");
        assert!(rules.contains(&"duplicate-name"), "{rules:?}");
        assert!(!rules.contains(&"broken-parent"));
    }

    #[test]
    fn fresh_project_is_clean() {
        let p = scene::new_project("x".into());
        assert!(check(&p).is_empty());
    }
}
