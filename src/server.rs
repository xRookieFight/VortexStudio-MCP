//! The MCP surface. Every tool loads the project fresh from disk, so edits
//! made in Studio between calls are never clobbered by a stale copy.

use std::path::{Path, PathBuf};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api;
use crate::check;
use crate::lint::{self, ScriptKind, Severity};
use crate::scene::{self, LightingProps, Props};
use crate::store;
use crate::sync;
use crate::vrtx::{Class, Project};

type ToolResult = Result<String, String>;

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn open(file: &str) -> Result<(PathBuf, Project), String> {
    let path = store::normalize(file)?;
    let project = store::load(&path)?;
    Ok((path, project))
}

fn save(path: &Path, project: &Project, mut summary: Value) -> ToolResult {
    let backup = store::save(path, project)?;
    summary["saved"] = json!(path.display().to_string());
    if let Some(b) = backup {
        summary["backup"] = json!(b.display().to_string());
    }
    if store::studio_running() {
        summary["warning"] = json!(
            "Vortex Studio is running. If it has this project open, reopen the project to see the changes and don't save from the old window, it would overwrite them."
        );
    }
    Ok(pretty(&summary))
}

fn lint_block(
    source: &str,
    kind: ScriptKind,
    allow_errors: bool,
) -> Result<Vec<lint::Diagnostic>, String> {
    let found = lint::lint(source, kind);
    let syntax: Vec<_> = found.iter().filter(|d| d.rule == "syntax").collect();
    if !syntax.is_empty() && !allow_errors {
        return Err(format!(
            "not saved, the code has syntax errors:\n{}\nFix them, or pass allow_errors: true to save anyway.",
            syntax
                .iter()
                .map(|d| format!("  line {}: {}", d.line, d.message))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Ok(found)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileArg {
    /// Path to the .vrtx project. ~ and Wine style Z:\\ paths work.
    pub file: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListArgs {
    pub file: String,
    /// Only show this instance and what's below it, e.g. "Workspace".
    pub root: Option<String>,
    /// How many levels to show below the root, default 8.
    pub depth: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InstanceArgs {
    pub file: String,
    /// Path like "Workspace/Model/Part" or index like "#12".
    pub instance: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetArgs {
    pub file: String,
    /// Path like "Workspace/Model/Part" or index like "#12".
    pub instance: String,
    /// Include script source, default false.
    pub include_source: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindArgs {
    pub file: String,
    /// Class name, e.g. "Part" or "Script".
    pub class: Option<String>,
    /// Case insensitive substring of the name.
    pub name_contains: Option<String>,
    /// Only search below this instance.
    pub under: Option<String>,
    /// Max results, default 100.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    pub file: String,
    /// Text to look for in script sources.
    pub pattern: String,
    /// Default false.
    pub case_sensitive: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryArgs {
    /// Name or words to look for, e.g. "Raycast", "tween", "Enum.KeyCode".
    pub query: String,
    /// Max results, default 25.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NameArg {
    /// A service, class, datatype, enum (with or without "Enum.") or library like "math".
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LintArgs {
    /// Luau source code.
    pub source: String,
    /// Where the code runs. Decides which client or server only APIs are allowed.
    pub kind: ScriptKind,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteScriptArgs {
    pub file: String,
    /// The script to overwrite, path or "#index".
    pub instance: String,
    /// Full new source code.
    pub source: String,
    /// Save even with syntax errors, default false.
    pub allow_errors: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateArgs {
    pub file: String,
    /// Where to put it, e.g. "Workspace" or "ServerScriptService".
    pub parent: String,
    /// Part, Model, Script, LocalScript, ModuleScript, RemoteEvent, BindableEvent, RemoteFunction or BindableFunction.
    pub class: String,
    pub name: String,
    /// Source code for script classes.
    pub source: Option<String>,
    /// Properties to set right away, same shape as set_properties.
    pub properties: Option<Props>,
    /// Save script source even with syntax errors, default false.
    pub allow_errors: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetArgs {
    pub file: String,
    pub instance: String,
    pub properties: Props,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenameArgs {
    pub file: String,
    pub instance: String,
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoveArgs {
    pub file: String,
    pub instance: String,
    /// New parent, path or "#index".
    pub new_parent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DuplicateArgs {
    pub file: String,
    pub instance: String,
    /// Parent for the copy, defaults to the original's parent.
    pub new_parent: Option<String>,
    /// Name for the copy, defaults to the original's name.
    pub name: Option<String>,
    /// Move the copy by [x, y, z] studs, applied to every part in it.
    pub offset: Option<[f32; 3]>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LightingArgs {
    pub file: String,
    pub lighting: LightingProps,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewProjectArgs {
    /// Where to create the .vrtx file.
    pub file: String,
    /// Replace an existing file (it gets backed up first), default false.
    pub overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DirArgs {
    pub file: String,
    /// Folder for the .luau files.
    pub dir: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RestoreArgs {
    pub file: String,
    /// Backup path from list_backups.
    pub backup: String,
}

#[derive(Clone)]
pub struct VortexStudio {
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

impl Default for VortexStudio {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl VortexStudio {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Summary of a .vrtx project: format version, instance counts by class, scripts and lighting.",
        annotations(read_only_hint = true)
    )]
    fn project_info(&self, Parameters(a): Parameters<FileArg>) -> ToolResult {
        let (path, p) = open(&a.file)?;
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for i in &p.instances {
            *counts.entry(i.class.to_string()).or_default() += 1;
        }
        let scripts: Vec<_> = p
            .instances
            .iter()
            .enumerate()
            .filter(|(_, i)| i.script.is_some())
            .map(|(n, i)| json!({ "path": scene::path_of(&p, n), "class": i.class.to_string(), "lines": i.script.as_ref().unwrap().source.lines().count() }))
            .collect();
        Ok(pretty(&json!({
            "file": path.display().to_string(),
            "format_version": p.version,
            "project_id": p.project_id,
            "instances": p.instances.len(),
            "classes": counts,
            "scripts": scripts,
            "lighting": scene::lighting_view(&p.lighting),
        })))
    }

    #[tool(
        description = "The instance tree as a flat list with depth, path and class. Use root to zoom in.",
        annotations(read_only_hint = true)
    )]
    fn list_instances(&self, Parameters(a): Parameters<ListArgs>) -> ToolResult {
        let (_, p) = open(&a.file)?;
        let root = a
            .root
            .as_deref()
            .map(|r| scene::resolve(&p, r))
            .transpose()?;
        Ok(pretty(&json!(scene::tree(&p, root, a.depth.unwrap_or(8)))))
    }

    #[tool(
        description = "Every property of one instance: transform, color, material, lights, flags, attributes, script info.",
        annotations(read_only_hint = true)
    )]
    fn get_instance(&self, Parameters(a): Parameters<GetArgs>) -> ToolResult {
        let (_, p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        Ok(pretty(&scene::instance_view(
            &p,
            idx,
            a.include_source.unwrap_or(false),
        )))
    }

    #[tool(
        description = "Find instances by class and/or name, optionally below a given instance.",
        annotations(read_only_hint = true)
    )]
    fn find_instances(&self, Parameters(a): Parameters<FindArgs>) -> ToolResult {
        let (_, p) = open(&a.file)?;
        let class = match &a.class {
            Some(c) => Some(Class::from_name(c).ok_or_else(|| format!("unknown class {c:?}"))?),
            None => None,
        };
        let scope: Option<Vec<usize>> = match &a.under {
            Some(u) => Some(scene::descendants(&p, scene::resolve(&p, u)?)),
            None => None,
        };
        let needle = a.name_contains.as_ref().map(|s| s.to_lowercase());
        let hits: Vec<Value> = p
            .instances
            .iter()
            .enumerate()
            .filter(|(i, inst)| {
                class.is_none_or(|c| inst.class == c)
                    && needle.as_ref().is_none_or(|n| inst.name.to_lowercase().contains(n))
                    && scope.as_ref().is_none_or(|s| s.contains(i))
            })
            .take(a.limit.unwrap_or(100))
            .map(|(i, inst)| json!({ "index": i, "path": scene::path_of(&p, i), "class": inst.class.to_string() }))
            .collect();
        Ok(pretty(&json!({ "count": hits.len(), "results": hits })))
    }

    #[tool(
        description = "Source code of a Script, LocalScript or ModuleScript.",
        annotations(read_only_hint = true)
    )]
    fn read_script(&self, Parameters(a): Parameters<InstanceArgs>) -> ToolResult {
        let (_, p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        let inst = &p.instances[idx];
        let s = inst
            .script
            .as_ref()
            .ok_or_else(|| format!("{} is a {}, not a script", inst.name, inst.class))?;
        Ok(format!(
            "-- {} ({}{})\n{}",
            scene::path_of(&p, idx),
            inst.class,
            if s.enabled { "" } else { ", disabled" },
            s.source
        ))
    }

    #[tool(
        description = "Search every script's source for text. Returns matching lines with line numbers.",
        annotations(read_only_hint = true)
    )]
    fn search_scripts(&self, Parameters(a): Parameters<SearchArgs>) -> ToolResult {
        let (_, p) = open(&a.file)?;
        if a.pattern.is_empty() {
            return Err("pattern is empty".into());
        }
        let cs = a.case_sensitive.unwrap_or(false);
        let needle = if cs {
            a.pattern.clone()
        } else {
            a.pattern.to_lowercase()
        };
        let mut hits = Vec::new();
        for (i, inst) in p.instances.iter().enumerate() {
            let Some(s) = &inst.script else { continue };
            for (n, line) in s.source.lines().enumerate() {
                let hay = if cs {
                    line.to_owned()
                } else {
                    line.to_lowercase()
                };
                if hay.contains(&needle) {
                    hits.push(json!({ "script": scene::path_of(&p, i), "line": n + 1, "text": line.trim() }));
                }
            }
        }
        Ok(pretty(&json!({ "count": hits.len(), "matches": hits })))
    }

    #[tool(
        description = "Health check for the whole project: broken structure, misplaced scripts, duplicate names and a Vortex lint of every script.",
        annotations(read_only_hint = true)
    )]
    fn check_project(&self, Parameters(a): Parameters<FileArg>) -> ToolResult {
        let (_, p) = open(&a.file)?;
        let findings = check::check(&p);
        let count = |s: Severity| findings.iter().filter(|f| f.severity == s).count();
        Ok(pretty(&json!({
            "errors": count(Severity::Error),
            "warnings": count(Severity::Warning),
            "info": count(Severity::Info),
            "findings": findings,
        })))
    }

    #[tool(
        description = "Search Vortex's Luau API: services, classes, datatypes, members, globals, libraries and enums.",
        annotations(read_only_hint = true)
    )]
    fn api_search(&self, Parameters(a): Parameters<QueryArgs>) -> ToolResult {
        let hits = api::get().search(&a.query, a.limit.unwrap_or(25));
        if hits.is_empty() {
            return Ok(format!(
                "nothing matches {:?}. If Roblox has it, Vortex most likely doesn't. Try api_overview for what exists.",
                a.query
            ));
        }
        Ok(pretty(&json!(hits)))
    }

    #[tool(
        description = "Full reference for one service, class, datatype, enum or library, including inherited members.",
        annotations(read_only_hint = true)
    )]
    fn api_get(&self, Parameters(a): Parameters<NameArg>) -> ToolResult {
        let api = api::get();
        let name = a.name.trim();
        if let Some((lib, funcs)) = api.libraries.get_key_value(name) {
            return Ok(pretty(&json!({ "library": lib, "functions": funcs })));
        }
        if let Some((e, items)) = api.find_enum(name)
            && (name.starts_with("Enum.") || api.find_type(name).is_empty())
        {
            return Ok(pretty(&json!({
                "enum": format!("Enum.{e}"),
                "items": items,
                "unverified_items": api.enums_unverified.get(e),
            })));
        }
        let found = api.find_type(name);
        if found.is_empty() {
            return Err(format!(
                "{name:?} isn't part of Vortex's API. Use api_search to look around."
            ));
        }
        let out: Vec<Value> = found
            .into_iter()
            .map(|(kind, t)| {
                let members: Vec<Value> = api
                    .members_with_inherited(t)
                    .into_iter()
                    .map(|(owner, m)| {
                        let mut v = serde_json::to_value(m).unwrap_or_default();
                        if owner != t.name {
                            v["inherited_from"] = json!(owner);
                        }
                        v
                    })
                    .collect();
                let mut v = serde_json::to_value(t).unwrap_or_default();
                v["kind"] = json!(kind);
                v["members"] = json!(members);
                v
            })
            .collect();
        Ok(pretty(&json!(out)))
    }

    #[tool(
        description = "What exists in Vortex at a glance: services, creatable classes, datatypes, enums, globals, library functions and engine limits.",
        annotations(read_only_hint = true)
    )]
    fn api_overview(&self) -> ToolResult {
        let api = api::get();
        Ok(pretty(&json!({
            "about": api.about,
            "services": api.services.iter().map(|s| &s.name).collect::<Vec<_>>(),
            "creatable_classes": api.classes.iter().filter(|c| c.creatable).map(|c| &c.name).collect::<Vec<_>>(),
            "other_classes": api.classes.iter().filter(|c| !c.creatable).map(|c| &c.name).collect::<Vec<_>>(),
            "datatypes": api.datatypes.iter().map(|d| &d.name).collect::<Vec<_>>(),
            "enums": api.enums.keys().collect::<Vec<_>>(),
            "globals": api.globals.iter().map(|g| &g.name).collect::<Vec<_>>(),
            "libraries": api.libraries,
            "limits": api.limits,
        })))
    }

    #[tool(
        description = "Lint Luau for Vortex: syntax errors, Roblox APIs Vortex lacks, unknown services, classes and enums, and client or server only APIs used on the wrong side.",
        annotations(read_only_hint = true)
    )]
    fn lint_luau(&self, Parameters(a): Parameters<LintArgs>) -> ToolResult {
        let found = lint::lint(&a.source, a.kind);
        if found.is_empty() {
            return Ok("no problems found".into());
        }
        Ok(pretty(&json!(found)))
    }

    #[tool(
        description = "Replace a script's source. The code is linted first and refused on syntax errors. Backs up the project.",
        annotations(destructive_hint = false)
    )]
    fn write_script(&self, Parameters(a): Parameters<WriteScriptArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        let kind = check::script_kind(p.instances[idx].class).ok_or_else(|| {
            format!(
                "{} is a {}, not a script",
                p.instances[idx].name, p.instances[idx].class
            )
        })?;
        let diagnostics = lint_block(&a.source, kind, a.allow_errors.unwrap_or(false))?;
        scene::set_source(&mut p, idx, &a.source)?;
        save(
            &path,
            &p,
            json!({ "updated": scene::path_of(&p, idx), "lint": diagnostics }),
        )
    }

    #[tool(
        description = "Create a Part, Model, script or Remote/Bindable object, optionally with properties and source."
    )]
    fn create_instance(&self, Parameters(a): Parameters<CreateArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let parent = scene::resolve(&p, &a.parent)?;
        let class =
            Class::from_name(&a.class).ok_or_else(|| format!("unknown class {:?}", a.class))?;
        let diagnostics = match (&a.source, check::script_kind(class)) {
            (Some(src), Some(kind)) => lint_block(src, kind, a.allow_errors.unwrap_or(false))?,
            _ => Vec::new(),
        };
        let idx = scene::create(&mut p, parent, class, &a.name, a.source.clone())?;
        let mut changed = Vec::new();
        if let Some(props) = &a.properties {
            changed = scene::set_props(&mut p, idx, props)?;
        }
        save(
            &path,
            &p,
            json!({ "created": scene::path_of(&p, idx), "index": idx, "properties_set": changed, "lint": diagnostics }),
        )
    }

    #[tool(
        description = "Change properties of an instance: transform, color, transparency, material, shape, physics flags, lights, textures, attributes, script enabled."
    )]
    fn set_properties(&self, Parameters(a): Parameters<SetArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        let changed = scene::set_props(&mut p, idx, &a.properties)?;
        save(
            &path,
            &p,
            json!({ "updated": scene::path_of(&p, idx), "changed": changed, "now": scene::instance_view(&p, idx, false) }),
        )
    }

    #[tool(description = "Rename an instance. Services can't be renamed.")]
    fn rename_instance(&self, Parameters(a): Parameters<RenameArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        let old = scene::path_of(&p, idx);
        scene::rename(&mut p, idx, &a.name)?;
        save(
            &path,
            &p,
            json!({ "renamed": old, "now": scene::path_of(&p, idx) }),
        )
    }

    #[tool(description = "Move an instance and everything below it under a new parent.")]
    fn move_instance(&self, Parameters(a): Parameters<MoveArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        let parent = scene::resolve(&p, &a.new_parent)?;
        let old = scene::path_of(&p, idx);
        let new_idx = scene::reparent(&mut p, idx, parent)?;
        save(
            &path,
            &p,
            json!({ "moved": old, "now": scene::path_of(&p, new_idx), "index": new_idx }),
        )
    }

    #[tool(
        description = "Copy an instance with everything below it, optionally renamed, re-parented or offset in space."
    )]
    fn duplicate_instance(&self, Parameters(a): Parameters<DuplicateArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        let parent = a
            .new_parent
            .as_deref()
            .map(|np| scene::resolve(&p, np))
            .transpose()?;
        let copy = scene::duplicate(&mut p, idx, parent)?;
        if let Some(name) = &a.name {
            scene::rename(&mut p, copy, name)?;
        }
        if let Some(off) = a.offset {
            if off.iter().any(|x| !x.is_finite()) {
                return Err("offset must be finite numbers".into());
            }
            let mut targets = vec![copy];
            targets.extend(scene::descendants(&p, copy));
            for t in targets {
                if let Some(part) = &mut p.instances[t].part {
                    for (pos, d) in part.position.iter_mut().zip(off) {
                        *pos += d;
                    }
                }
            }
        }
        save(
            &path,
            &p,
            json!({ "copy": scene::path_of(&p, copy), "index": copy }),
        )
    }

    #[tool(
        description = "Delete an instance and everything below it. Services can't be deleted. The project is backed up first.",
        annotations(destructive_hint = true)
    )]
    fn delete_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let idx = scene::resolve(&p, &a.instance)?;
        let what = scene::path_of(&p, idx);
        let removed = scene::delete(&mut p, idx)?;
        save(
            &path,
            &p,
            json!({ "deleted": what, "instances_removed": removed }),
        )
    }

    #[tool(
        description = "Change ambient light, brightness, sun color, strength, shadows and direction."
    )]
    fn set_lighting(&self, Parameters(a): Parameters<LightingArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let changed = scene::set_lighting(&mut p.lighting, &a.lighting)?;
        save(
            &path,
            &p,
            json!({ "changed": changed, "lighting": scene::lighting_view(&p.lighting) }),
        )
    }

    #[tool(
        description = "Create a new project identical to Studio's empty template: the services and a baseplate."
    )]
    fn new_project(&self, Parameters(a): Parameters<NewProjectArgs>) -> ToolResult {
        let path = store::normalize(&a.file)?;
        if path.extension().is_none_or(|e| e != "vrtx") {
            return Err("the file name should end in .vrtx so Studio can open it".into());
        }
        if path.exists() && !a.overwrite.unwrap_or(false) {
            return Err(format!(
                "{} already exists, pass overwrite: true to replace it",
                path.display()
            ));
        }
        let p = scene::new_project(new_id());
        save(&path, &p, json!({ "created": path.display().to_string() }))
    }

    #[tool(
        description = "Write every script to a folder as .luau files (Name.server.luau, Name.client.luau, Name.luau) plus an index, for editing outside Studio.",
        annotations(read_only_hint = true)
    )]
    fn export_scripts(&self, Parameters(a): Parameters<DirArgs>) -> ToolResult {
        let (_, p) = open(&a.file)?;
        let dir = store::normalize(&a.dir)?;
        let index = sync::export(&p, &dir)?;
        Ok(pretty(&json!({
            "dir": dir.display().to_string(),
            "files": index.scripts.keys().collect::<Vec<_>>(),
        })))
    }

    #[tool(
        description = "Copy edited .luau files from an export_scripts folder back into the project. Lints each script, backs up the project."
    )]
    fn import_scripts(&self, Parameters(a): Parameters<DirArgs>) -> ToolResult {
        let (path, mut p) = open(&a.file)?;
        let dir = store::normalize(&a.dir)?;
        let report = sync::import(&mut p, &dir)?;
        let mut lint_findings = Vec::new();
        for (i, inst) in p.instances.iter().enumerate() {
            if report.updated.contains(&scene::path_of(&p, i))
                && let (Some(kind), Some(s)) = (check::script_kind(inst.class), &inst.script)
            {
                for d in lint::lint(&s.source, kind) {
                    lint_findings.push(json!({ "script": scene::path_of(&p, i), "diagnostic": d }));
                }
            }
        }
        if lint_findings
            .iter()
            .any(|f| f["diagnostic"]["rule"] == "syntax")
        {
            return Err(format!(
                "not saved, imported scripts have syntax errors:\n{}",
                serde_json::to_string_pretty(&lint_findings).unwrap_or_default()
            ));
        }
        if report.updated.is_empty() {
            return Ok(pretty(
                &json!({ "report": report, "note": "nothing changed, project not touched" }),
            ));
        }
        save(
            &path,
            &p,
            json!({ "report": report, "lint": lint_findings }),
        )
    }

    #[tool(
        description = "Backups this tool made of a project, newest first.",
        annotations(read_only_hint = true)
    )]
    fn list_backups(&self, Parameters(a): Parameters<FileArg>) -> ToolResult {
        let path = store::normalize(&a.file)?;
        let list: Vec<String> = store::list_backups(&path)
            .iter()
            .map(|b| b.display().to_string())
            .collect();
        Ok(pretty(&json!({ "count": list.len(), "backups": list })))
    }

    #[tool(
        description = "Put a backup back in place of the project. The current file is backed up too, so this can be undone."
    )]
    fn restore_backup(&self, Parameters(a): Parameters<RestoreArgs>) -> ToolResult {
        let path = store::normalize(&a.file)?;
        let backup = store::normalize(&a.backup)?;
        if !store::list_backups(&path).contains(&backup) {
            return Err("that isn't one of this project's backups, see list_backups".into());
        }
        let p = store::load(&backup)?;
        save(
            &path,
            &p,
            json!({ "restored_from": backup.display().to_string() }),
        )
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "vortexstudio-mcp",
    instructions = "Tools for Vortex Studio projects (.vrtx files) and Vortex's Luau API. \
Instances are addressed by path from a service, like Workspace/Model/Part, or by index like #12 when names repeat; list_instances shows them. \
Vortex supports only a subset of the Roblox API: check names with api_search or api_get before writing Luau, and run lint_luau on new code. \
write_script and create_instance lint for you and refuse code with syntax errors. \
Every edit saves a backup in .vrtx-backups next to the project. If Studio has the project open, tell the user to reopen it after edits and not to save from the old window, or it will overwrite the changes."
)]
impl ServerHandler for VortexStudio {}

/// 32 hex chars like Studio's project ids. Not cryptographic, it only has to be unique.
fn new_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for salt in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call<T>(
        f: impl FnOnce(&VortexStudio, Parameters<T>) -> ToolResult,
        args: Value,
    ) -> ToolResult
    where
        T: serde::de::DeserializeOwned,
    {
        let s = VortexStudio::new();
        f(&s, Parameters(serde_json::from_value(args).unwrap()))
    }

    #[test]
    fn end_to_end_edit_session() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("game.vrtx").display().to_string();

        call(VortexStudio::new_project, json!({ "file": file })).unwrap();
        assert!(call(VortexStudio::new_project, json!({ "file": file })).is_err());

        let made = call(
            VortexStudio::create_instance,
            json!({
                "file": file, "parent": "Workspace", "class": "Part", "name": "Lava",
                "properties": { "color": "#FF3300", "position": [0, 1, 10], "material": "Metal" }
            }),
        )
        .unwrap();
        assert!(made.contains("Workspace/Lava"), "{made}");

        let bad = call(
            VortexStudio::create_instance,
            json!({ "file": file, "parent": "ServerScriptService", "class": "Script", "name": "Kill", "source": "local x =" }),
        );
        assert!(bad.unwrap_err().contains("syntax"));

        call(
            VortexStudio::create_instance,
            json!({
                "file": file, "parent": "ServerScriptService", "class": "Script", "name": "Kill",
                "source": "workspace.Lava.Touched:Connect(function(hit) end)\n"
            }),
        )
        .unwrap();

        let tree = call(VortexStudio::list_instances, json!({ "file": file })).unwrap();
        assert!(tree.contains("ServerScriptService/Kill"));

        call(VortexStudio::duplicate_instance, json!({ "file": file, "instance": "Workspace/Lava", "name": "Lava2", "offset": [5, 0, 0] })).unwrap();
        let info = call(
            VortexStudio::get_instance,
            json!({ "file": file, "instance": "Workspace/Lava2" }),
        )
        .unwrap();
        assert!(info.contains("\"position\": [\n      5.0"), "{info}");

        call(
            VortexStudio::delete_instance,
            json!({ "file": file, "instance": "Workspace/Lava" }),
        )
        .unwrap();
        let backups = call(VortexStudio::list_backups, json!({ "file": file })).unwrap();
        // new_project had nothing to back up and the refused script never saved
        assert!(backups.contains("\"count\": 4"), "{backups}");

        let check = call(VortexStudio::check_project, json!({ "file": file })).unwrap();
        assert!(check.contains("\"errors\": 0"), "{check}");
    }

    #[test]
    fn api_tools_answer() {
        let s = VortexStudio::new();
        assert!(
            s.api_get(Parameters(NameArg {
                name: "Part".into()
            }))
            .unwrap()
            .contains("Anchored")
        );
        assert!(
            s.api_get(Parameters(NameArg {
                name: "Enum.KeyCode".into()
            }))
            .unwrap()
            .contains("Space")
        );
        assert!(
            s.api_get(Parameters(NameArg {
                name: "math".into()
            }))
            .unwrap()
            .contains("clamp")
        );
        assert!(
            s.api_get(Parameters(NameArg {
                name: "HttpService".into()
            }))
            .is_err()
        );
        assert!(s.api_overview().unwrap().contains("TweenService"));
    }

    #[test]
    fn ids_look_like_studios() {
        let id = new_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(id, new_id());
    }
}
