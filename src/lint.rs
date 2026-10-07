//! Luau checks tuned for Vortex.
//!
//! Syntax comes from full_moon's Luau parser. The Vortex rules work on the
//! token stream instead of the AST: they look for short, unambiguous shapes
//! like `GetService("X")` or `Enum.A.B`, which is all they need and keeps them
//! robust against code the parser only half understands.

use full_moon::LuaVersion;
use full_moon::node::Node;
use full_moon::tokenizer::TokenType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum ScriptKind {
    /// Server side, a `Script`.
    Script,
    /// Client side, a `LocalScript`.
    LocalScript,
    /// Shared code, a `ModuleScript`. Side specific checks are skipped.
    ModuleScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub rule: &'static str,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
struct Tok {
    text: String,
    kind: Kind,
    line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Ident,
    Str,
    Symbol,
    Other,
}

// services people reach for out of Roblox habit that Vortex doesn't have
const ROBLOX_ONLY_SERVICES: &[&str] = &[
    "DataStoreService",
    "HttpService",
    "MarketplaceService",
    "TeleportService",
    "SoundService",
    "PathfindingService",
    "CollectionService",
    "ContextActionService",
    "StarterGui",
    "StarterPack",
    "Teams",
    "Chat",
    "TextChatService",
    "BadgeService",
    "MessagingService",
    "MemoryStoreService",
    "PhysicsService",
    "ServerStorage",
    "GuiService",
    "ProximityPromptService",
    "SocialService",
    "LocalizationService",
];

pub fn lint(source: &str, kind: ScriptKind) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let parsed = full_moon::parse_fallible(source, LuaVersion::luau());
    for err in parsed.errors() {
        let (start, _) = err.range();
        out.push(Diagnostic {
            severity: Severity::Error,
            rule: "syntax",
            line: start.line(),
            message: err.error_message().into_owned(),
        });
    }

    // Node::tokens walks containers bracket first, so put them back in source order
    let mut raw: Vec<_> = parsed
        .ast()
        .tokens()
        .filter(|t| !t.token_type().is_trivia())
        .collect();
    raw.sort_by_key(|t| t.token().start_position().bytes());
    let toks: Vec<Tok> = raw
        .into_iter()
        .filter_map(|t| {
            let line = t.token().start_position().line();
            let (text, kind) = match t.token_type() {
                TokenType::Identifier { identifier } => (identifier.to_string(), Kind::Ident),
                TokenType::StringLiteral { literal, .. } => (literal.to_string(), Kind::Str),
                TokenType::Symbol { .. } => (t.token().to_string(), Kind::Symbol),
                TokenType::Eof => return None,
                _ => (t.token().to_string(), Kind::Other),
            };
            Some(Tok { text, kind, line })
        })
        .collect();

    check_tokens(&toks, kind, &mut out);
    out.sort_by(|a, b| a.line.cmp(&b.line).then(b.severity.cmp(&a.severity)));
    out.dedup();
    out
}

fn is(t: Option<&Tok>, kind: Kind, text: &str) -> bool {
    t.is_some_and(|t| t.kind == kind && t.text == text)
}

/// The string argument of a call starting at `i` (the token after the callee),
/// for both `f("x")` and `f "x"`.
fn string_arg(toks: &[Tok], i: usize) -> Option<&Tok> {
    match toks.get(i) {
        Some(t) if t.kind == Kind::Str => Some(t),
        Some(t) if t.kind == Kind::Symbol && t.text == "(" => {
            toks.get(i + 1).filter(|t| t.kind == Kind::Str)
        }
        _ => None,
    }
}

fn check_tokens(toks: &[Tok], kind: ScriptKind, out: &mut Vec<Diagnostic>) {
    let api = api::get();
    let services = api.service_names();
    let creatable = api.creatable_classes();
    let mut push = |severity, rule, line, message: String| {
        out.push(Diagnostic {
            severity,
            rule,
            line,
            message,
        });
    };

    for (i, t) in toks.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| toks.get(p));
        let after_member_access =
            prev.is_some_and(|p| p.kind == Kind::Symbol && (p.text == "." || p.text == ":"));

        if t.kind != Kind::Ident {
            continue;
        }

        // game:GetService("Name")
        if t.text == "GetService"
            && is(prev, Kind::Symbol, ":")
            && let Some(arg) = string_arg(toks, i + 1)
        {
            let name = arg.text.as_str();
            if ROBLOX_ONLY_SERVICES.contains(&name) {
                push(
                    Severity::Error,
                    "unknown-service",
                    arg.line,
                    format!("{name} exists in Roblox but not in Vortex"),
                );
            } else if !services.contains(name) {
                push(
                    Severity::Warning,
                    "unknown-service",
                    arg.line,
                    format!(
                        "{name} isn't a known Vortex service. Known: {}",
                        sorted(&services).join(", ")
                    ),
                );
            }
            if kind == ScriptKind::Script && name == "UserInputService" {
                push(
                    Severity::Warning,
                    "client-only",
                    arg.line,
                    "UserInputService only works in LocalScripts".into(),
                );
            }
        }

        // Instance.new("Class")
        if t.text == "Instance"
            && is(toks.get(i + 1), Kind::Symbol, ".")
            && is(toks.get(i + 2), Kind::Ident, "new")
            && let Some(arg) = string_arg(toks, i + 3)
            && !creatable.contains(arg.text.as_str())
        {
            push(
                Severity::Error,
                "unknown-class",
                arg.line,
                format!(
                    "Instance.new can't create {:?} in Vortex. Creatable: {}",
                    arg.text,
                    sorted(&creatable).join(", ")
                ),
            );
        }

        // Enum.Type.Item
        if t.text == "Enum"
            && !after_member_access
            && is(toks.get(i + 1), Kind::Symbol, ".")
            && let Some(ty) = toks.get(i + 2).filter(|x| x.kind == Kind::Ident)
        {
            match api.find_enum(&ty.text) {
                None => push(
                    Severity::Error,
                    "unknown-enum",
                    ty.line,
                    format!("Enum.{} doesn't exist in Vortex", ty.text),
                ),
                Some((name, items)) => {
                    if is(toks.get(i + 3), Kind::Symbol, ".")
                        && let Some(item) = toks.get(i + 4).filter(|x| x.kind == Kind::Ident)
                    {
                        let known = items.contains(&item.text)
                            || api
                                .enums_unverified
                                .get(name)
                                .is_some_and(|u| u.contains(&item.text))
                            || matches!(
                                item.text.as_str(),
                                "GetEnumItems" | "FromName" | "FromValue"
                            );
                        if !known {
                            push(
                                Severity::Error,
                                "unknown-enum",
                                item.line,
                                format!(
                                    "Enum.{name}.{} isn't a valid item. Valid: {}",
                                    item.text,
                                    items.join(", ")
                                ),
                            );
                        }
                    }
                }
            }
        }

        // library and datatype statics: math.foo, Vector3.foo, Color3.fromHSV ...
        if !after_member_access
            && is(toks.get(i + 1), Kind::Symbol, ".")
            && let Some(member) = toks.get(i + 2).filter(|x| x.kind == Kind::Ident)
        {
            if let Some(funcs) = api.libraries.get(&t.text) {
                if !funcs.contains(&member.text) {
                    push(
                        Severity::Error,
                        "unknown-library-member",
                        member.line,
                        format!(
                            "{}.{} isn't available in Vortex's Luau",
                            t.text, member.text
                        ),
                    );
                }
            } else if let Some(dt) = api.datatypes.iter().find(|d| d.name == t.text) {
                let static_ok = dt.members.iter().any(|m| {
                    m.name == member.text && matches!(m.kind.as_str(), "constructor" | "constant")
                });
                if !static_ok {
                    push(
                        Severity::Error,
                        "unknown-datatype-member",
                        member.line,
                        format!("{}.{} doesn't exist in Vortex", t.text, member.text),
                    );
                }
            }
        }

        // deprecated globals: wait(), spawn(), delay()
        if !after_member_access
            && matches!(t.text.as_str(), "wait" | "spawn" | "delay")
            && is(toks.get(i + 1), Kind::Symbol, "(")
            // keywords come through as symbols
            && !is(prev, Kind::Symbol, "function")
            && !is(prev, Kind::Symbol, "local")
        {
            push(
                Severity::Info,
                "prefer-task",
                t.line,
                format!("use task.{} instead of the old global {}()", t.text, t.text),
            );
        }

        // side checks
        match kind {
            ScriptKind::Script => {
                if t.text == "LocalPlayer" && after_member_access {
                    push(
                        Severity::Error,
                        "client-only",
                        t.line,
                        "Players.LocalPlayer is nil on the server. Use Players.PlayerAdded or the player argument of OnServerEvent".into(),
                    );
                }
                if after_member_access
                    && matches!(
                        t.text.as_str(),
                        "FireServer"
                            | "InvokeServer"
                            | "OnClientEvent"
                            | "OnClientInvoke"
                            | "RenderStepped"
                    )
                {
                    push(
                        Severity::Error,
                        "client-only",
                        t.line,
                        format!("{} only works in LocalScripts", t.text),
                    );
                }
            }
            ScriptKind::LocalScript => {
                if after_member_access
                    && matches!(
                        t.text.as_str(),
                        "FireClient"
                            | "FireAllClients"
                            | "InvokeClient"
                            | "OnServerEvent"
                            | "OnServerInvoke"
                    )
                {
                    push(
                        Severity::Error,
                        "server-only",
                        t.line,
                        format!("{} only works in server Scripts", t.text),
                    );
                }
            }
            ScriptKind::ModuleScript => {}
        }
    }

    // module scripts must hand something back to require()
    if kind == ScriptKind::ModuleScript
        && !toks
            .iter()
            .any(|t| t.kind == Kind::Symbol && t.text == "return")
    {
        out.push(Diagnostic {
            severity: Severity::Warning,
            rule: "module-return",
            line: toks.last().map_or(1, |t| t.line),
            message: "ModuleScripts should end with a return, require() gets nil otherwise".into(),
        });
    }
}

fn sorted<'a>(set: &std::collections::HashSet<&'a str>) -> Vec<&'a str> {
    let mut v: Vec<&str> = set.iter().copied().collect();
    v.sort_unstable();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(src: &str, kind: ScriptKind) -> Vec<&'static str> {
        lint(src, kind).into_iter().map(|d| d.rule).collect()
    }

    #[test]
    fn clean_code_has_no_findings() {
        let src = r#"
local Players = game:GetService("Players")
local TweenService = game:GetService("TweenService")
local part = Instance.new("Part")
part.Material = Enum.Material.Wood
part.Color = Color3.fromRGB(255, 0, 0)
part.CFrame = CFrame.new(0, 5, 0) * CFrame.Angles(0, math.rad(45), 0)
local info = TweenInfo.new(1, Enum.EasingStyle.Quad, Enum.EasingDirection.Out)
TweenService:Create(part, info, { Transparency = 1 }):Play()
Players.PlayerAdded:Connect(function(player)
    print(player.DisplayName, string.upper("hi"), table.concat({ "a" }, ","))
end)
task.wait(1)
"#;
        let found = lint(src, ScriptKind::Script);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn syntax_errors_have_lines() {
        let found = lint("local x = \nprint(", ScriptKind::Script);
        assert!(
            found
                .iter()
                .any(|d| d.rule == "syntax" && d.severity == Severity::Error)
        );
    }

    #[test]
    fn roblox_only_apis_are_flagged() {
        let src = r#"
local ds = game:GetService("DataStoreService")
local s = Instance.new("Sound")
local c = Color3.fromHSV(0.5, 1, 1)
local m = Enum.Material.Neon
local x = math.nope(1)
"#;
        let r = rules(src, ScriptKind::Script);
        for rule in [
            "unknown-service",
            "unknown-class",
            "unknown-datatype-member",
            "unknown-enum",
            "unknown-library-member",
        ] {
            assert!(r.contains(&rule), "missing {rule} in {r:?}");
        }
    }

    #[test]
    fn side_rules() {
        let server = "local p = game:GetService(\"Players\").LocalPlayer\nremote:FireServer(1)";
        assert_eq!(
            rules(server, ScriptKind::Script),
            ["client-only", "client-only"]
        );
        let client = "remote.OnServerEvent:Connect(print)\nremote:FireAllClients()";
        assert_eq!(
            rules(client, ScriptKind::LocalScript),
            ["server-only", "server-only"]
        );
        assert!(rules(server, ScriptKind::ModuleScript).contains(&"module-return"));
    }

    #[test]
    fn old_wait_gets_a_hint_but_methods_dont() {
        assert_eq!(rules("wait(1)", ScriptKind::Script), ["prefer-task"]);
        assert!(
            rules(
                "signal:Wait()\nlocal function wait(x) end",
                ScriptKind::Script
            )
            .is_empty()
        );
    }

    #[test]
    fn community_style_code_is_clean() {
        // trimmed from a published Vortex project (VortexUI by Runtem)
        let src = r#"
local RunService = game:GetService("RunService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Module = ReplicatedStorage:WaitForChild("VortexFont")
local SendBillboards = ReplicatedStorage:WaitForChild("SendBillboards") :: RemoteEvent
export type Props = { origin: Vector3, align: ("left" | "center")? }
SendBillboards.OnClientEvent:Connect(function(billboards)
    for id, new in pairs(billboards) do end
end)
RunService.Heartbeat:Connect(function() end)
"#;
        let found = lint(src, ScriptKind::LocalScript);
        assert!(found.is_empty(), "{found:#?}");
    }
}
