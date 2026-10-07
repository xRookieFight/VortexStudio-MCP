//! Moves script sources between a project and a folder of `.luau` files, so
//! they can be edited with a normal editor and tracked in git. Layout follows
//! Rojo: `Name.server.luau`, `Name.client.luau` and `Name.luau` for modules.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::scene;
use crate::vrtx::{Class, Project};

pub const INDEX_FILE: &str = "vortex-scripts.json";

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub index: usize,
    pub path: String,
    pub class: String,
}

/// Maps file paths relative to the export folder to the script they came from.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    pub project_id: Option<String>,
    pub scripts: BTreeMap<String, Entry>,
}

fn suffix(class: Class) -> &'static str {
    match class {
        Class::Script => ".server.luau",
        Class::LocalScript => ".client.luau",
        _ => ".luau",
    }
}

// keep names readable but safe on every filesystem
fn clean(segment: &str) -> String {
    let mut s: String = segment
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if s.is_empty() || s.chars().all(|c| c == '.') {
        s = format!("_{s}");
    }
    s
}

pub fn export(p: &Project, dir: &Path) -> Result<Index> {
    fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let mut index = Index {
        project_id: p.project_id.clone(),
        ..Default::default()
    };
    for (i, inst) in p.instances.iter().enumerate() {
        let Some(script) = &inst.script else { continue };
        let path = scene::path_of(p, i);
        let mut segments: Vec<String> = path.split('/').map(clean).collect();
        let name = segments.pop().unwrap_or_default();
        let mut rel = segments.join("/");
        if !rel.is_empty() {
            rel.push('/');
        }
        let mut file = format!("{rel}{name}{}", suffix(inst.class));
        // two scripts with the same path get the index appended
        if index.scripts.contains_key(&file) {
            file = format!("{rel}{name}~{i}{}", suffix(inst.class));
        }
        let full = dir.join(&file);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("can't create {}: {e}", parent.display()))?;
        }
        fs::write(&full, &script.source)
            .map_err(|e| format!("can't write {}: {e}", full.display()))?;
        index.scripts.insert(
            file,
            Entry {
                index: i,
                path,
                class: inst.class.to_string(),
            },
        );
    }
    let json = serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?;
    fs::write(dir.join(INDEX_FILE), json).map_err(|e| format!("can't write the index: {e}"))?;
    Ok(index)
}

#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    pub updated: Vec<String>,
    pub unchanged: usize,
    pub skipped: Vec<String>,
    pub untracked_files: Vec<String>,
}

/// Copies edited sources back. Entries whose instance moved, got renamed or
/// changed class since the export are skipped rather than guessed at.
pub fn import(p: &mut Project, dir: &Path) -> Result<ImportReport> {
    let raw = fs::read_to_string(dir.join(INDEX_FILE)).map_err(|e| {
        format!(
            "no {INDEX_FILE} in {}, run export_scripts first ({e})",
            dir.display()
        )
    })?;
    let index: Index =
        serde_json::from_str(&raw).map_err(|e| format!("{INDEX_FILE} is broken: {e}"))?;
    if index.project_id.is_some() && index.project_id != p.project_id {
        return Err("these scripts were exported from a different project".into());
    }
    let mut report = ImportReport::default();
    for (file, entry) in &index.scripts {
        let rel = Path::new(file);
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            report
                .skipped
                .push(format!("{file}: path escapes the folder"));
            continue;
        }
        let ok = p.instances.get(entry.index).is_some_and(|inst| {
            inst.class.to_string() == entry.class && scene::path_of(p, entry.index) == entry.path
        });
        if !ok {
            report.skipped.push(format!(
                "{file}: {} moved, was renamed or deleted since the export",
                entry.path
            ));
            continue;
        }
        let source = match fs::read_to_string(dir.join(rel)) {
            Ok(s) => s,
            Err(e) => {
                report.skipped.push(format!("{file}: {e}"));
                continue;
            }
        };
        let script = p.instances[entry.index]
            .script
            .as_mut()
            .expect("class checked above");
        if script.source == source {
            report.unchanged += 1;
        } else {
            script.source = source;
            report.updated.push(entry.path.clone());
        }
    }
    report.untracked_files = luau_files(dir)
        .into_iter()
        .filter(|f| !index.scripts.contains_key(f))
        .collect();
    Ok(report)
}

fn luau_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let Ok(entries) = fs::read_dir(dir.join(&rel)) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let child = rel.join(&name);
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(child),
                Ok(t) if t.is_file() && name.ends_with(".luau") => {
                    out.push(child.to_string_lossy().replace('\\', "/"));
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vrtx;

    #[test]
    fn export_edit_import() {
        let mut p = vrtx::decode(include_bytes!("../tests/fixtures/showcase.vrtx")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let index = export(&p, dir.path()).unwrap();
        assert_eq!(index.scripts.len(), 3);
        assert!(
            index
                .scripts
                .contains_key("ServerScriptService/Script.server.luau")
        );
        assert!(
            index
                .scripts
                .contains_key("ReplicatedStorage/LocalScript.client.luau")
        );
        assert!(
            index
                .scripts
                .contains_key("ReplicatedStorage/ModuleScript.luau")
        );

        fs::write(
            dir.path().join("ServerScriptService/Script.server.luau"),
            "print(\"edited\")\n",
        )
        .unwrap();
        fs::write(dir.path().join("extra.luau"), "-- new").unwrap();
        let report = import(&mut p, dir.path()).unwrap();
        assert_eq!(report.updated, ["ServerScriptService/Script"]);
        assert_eq!(report.unchanged, 2);
        assert_eq!(report.untracked_files, ["extra.luau"]);
        assert_eq!(
            p.instances[21].script.as_ref().unwrap().source,
            "print(\"edited\")\n"
        );
    }

    #[test]
    fn renamed_scripts_are_skipped_not_guessed() {
        let mut p = vrtx::decode(include_bytes!("../tests/fixtures/showcase.vrtx")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        export(&p, dir.path()).unwrap();
        scene::rename(&mut p, 21, "Main").unwrap();
        let report = import(&mut p, dir.path()).unwrap();
        assert_eq!(report.skipped.len(), 1);
    }

    #[test]
    fn hostile_names_stay_inside_the_folder() {
        assert_eq!(clean("../../etc"), ".._.._etc");
        assert_eq!(clean(".."), "_..");
        assert_eq!(clean("a:b*c"), "a_b_c");
    }

    #[test]
    fn index_paths_cant_escape() {
        let mut p = scene::new_project("x".into());
        let dir = tempfile::tempdir().unwrap();
        let mut index = Index {
            project_id: Some("x".into()),
            ..Default::default()
        };
        index.scripts.insert(
            "../evil.luau".into(),
            Entry {
                index: 5,
                path: "Workspace/Baseplate".into(),
                class: "Part".into(),
            },
        );
        fs::write(
            dir.path().join(INDEX_FILE),
            serde_json::to_string(&index).unwrap(),
        )
        .unwrap();
        let report = import(&mut p, dir.path()).unwrap();
        assert!(report.skipped[0].contains("escapes"));
    }
}
