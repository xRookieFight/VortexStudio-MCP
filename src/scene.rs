//! Friendly view of a project's instance tree and the edits we allow on it.
//!
//! Instances live in one flat list and point at their parent by index. Studio
//! always writes parents before their children, so every edit here keeps that
//! true: new and moved subtrees go to the end of the list.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::vrtx::{
    AttrValue, Class, Instance, Lighting, Part, PointLight, Project, Script, SpotLight, Texture,
};

pub const MATERIALS: [&str; 7] = [
    "SmoothPlastic",
    "Plastic",
    "Wood",
    "Metal",
    "Grass",
    "Ice",
    "Paint",
];
pub const SHAPES: [&str; 5] = ["Block", "Wedge", "CornerWedge", "Cylinder", "Ball"];
pub const FACES: [&str; 6] = ["Front", "Back", "Top", "Bottom", "Left", "Right"];
pub const SURFACES: [&str; 2] = ["Studs", "Inlets"];

// Studio's "white" is one ulp short of 1.0, presumably from an sRGB round trip
const NEAR_WHITE: f32 = f32::from_bits(0x3F7F_FFFF);

/// Classes we can create from scratch. The rest need data we haven't seen in
/// a real file yet, so creating them could produce something Studio rejects.
pub const CREATABLE: [Class; 9] = [
    Class::Part,
    Class::Model,
    Class::Script,
    Class::LocalScript,
    Class::ModuleScript,
    Class::RemoteEvent,
    Class::BindableEvent,
    Class::RemoteFunction,
    Class::BindableFunction,
];

type Result<T> = std::result::Result<T, String>;

fn enum_name(table: &[&str], id: u32) -> Value {
    match table.get(id as usize) {
        Some(name) => json!(name),
        None => json!(format!("Unknown({id})")),
    }
}

fn enum_id(table: &[&str], what: &str, name: &str) -> Result<u32> {
    let name = name.rsplit('.').next().unwrap_or(name);
    table
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|i| i as u32)
        .ok_or_else(|| {
            format!(
                "unknown {what} {name:?}, expected one of {}",
                table.join(", ")
            )
        })
}

// ---------- tree navigation ----------

pub fn children(p: &Project, idx: usize) -> Vec<usize> {
    p.instances
        .iter()
        .enumerate()
        .filter(|(_, i)| i.parent == Some(idx as u64))
        .map(|(n, _)| n)
        .collect()
}

pub fn descendants(p: &Project, idx: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![idx];
    while let Some(n) = stack.pop() {
        for c in children(p, n) {
            out.push(c);
            stack.push(c);
        }
    }
    out.sort_unstable();
    out
}

/// Slash separated path from the service down, e.g. `Workspace/Model/Part`.
pub fn path_of(p: &Project, idx: usize) -> String {
    let mut parts = Vec::new();
    let mut cur = Some(idx);
    let mut guard = 0;
    while let Some(n) = cur {
        let Some(inst) = p.instances.get(n) else {
            parts.push(format!("<missing #{n}>"));
            break;
        };
        parts.push(inst.name.clone());
        cur = inst.parent.map(|x| x as usize);
        guard += 1;
        if guard > p.instances.len() {
            parts.push("<cycle>".into());
            break;
        }
    }
    parts.reverse();
    parts.join("/")
}

/// Accepts `#12` for an index or a path like `Workspace/Model/Part`. A path
/// that matches several instances is an error listing them, so callers can retry by index.
pub fn resolve(p: &Project, spec: &str) -> Result<usize> {
    let spec = spec.trim();
    if let Some(n) = spec.strip_prefix('#') {
        let n: usize = n
            .parse()
            .map_err(|_| format!("bad instance index {spec:?}"))?;
        return if n < p.instances.len() {
            Ok(n)
        } else {
            Err(format!(
                "there's no instance #{n}, the project has {}",
                p.instances.len()
            ))
        };
    }
    let wanted = spec.trim_matches('/');
    let matches: Vec<usize> = (0..p.instances.len())
        .filter(|&i| path_of(p, i) == wanted)
        .collect();
    match matches.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!(
            "no instance at {wanted:?}. Paths start at a service, e.g. Workspace/Baseplate. Use list_instances to see the tree"
        )),
        many => Err(format!(
            "{wanted:?} matches {} instances, pick one by index: {}",
            many.len(),
            many.iter()
                .map(|i| format!("#{i}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn tree(p: &Project, root: Option<usize>, max_depth: usize) -> Vec<Value> {
    fn walk(p: &Project, idx: usize, depth: usize, max: usize, out: &mut Vec<Value>) {
        let inst = &p.instances[idx];
        let kids = children(p, idx);
        let mut line = json!({
            "index": idx,
            "depth": depth,
            "path": path_of(p, idx),
            "class": inst.class.to_string(),
        });
        if let Some(s) = &inst.script {
            line["lines"] = json!(s.source.lines().count());
        }
        if depth == max && !kids.is_empty() {
            line["hidden_children"] = json!(kids.len());
        }
        out.push(line);
        if depth < max {
            for c in kids {
                walk(p, c, depth + 1, max, out);
            }
        }
    }
    let mut out = Vec::new();
    match root {
        Some(r) => walk(p, r, 0, max_depth, &mut out),
        None => {
            for (i, inst) in p.instances.iter().enumerate() {
                if inst.parent.is_none() {
                    walk(p, i, 0, max_depth, &mut out);
                }
            }
        }
    }
    out
}

// ---------- views ----------

pub fn hex(c: [f32; 4]) -> String {
    let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

fn round(x: f32) -> f64 {
    (x as f64 * 1000.0).round() / 1000.0
}

fn v(xs: &[f32]) -> Value {
    json!(xs.iter().map(|x| round(*x)).collect::<Vec<_>>())
}

fn point_light_view(l: &PointLight) -> Value {
    json!({ "color": hex(l.color), "brightness": round(l.intensity), "range": round(l.range) })
}

fn spot_light_view(l: &SpotLight) -> Value {
    json!({
        "color": hex(l.color), "brightness": round(l.intensity), "range": round(l.range),
        "angle": round(l.angle), "face": enum_name(&FACES, l.face),
    })
}

pub fn part_view(part: &Part) -> Value {
    json!({
        "position": v(&part.position),
        "size": v(&part.scale),
        "rotation_degrees": v(&quat_to_euler_deg(part.rotation)),
        "rotation_quaternion": v(&part.rotation),
        "color": hex(part.color),
        "transparency": round(1.0 - part.color[3]),
        "material": enum_name(&MATERIALS, part.material),
        "shape": enum_name(&SHAPES, part.shape),
        "anchored": part.anchored,
        "can_collide": part.can_collide,
        "cast_shadow": part.cast_shadow,
        "spawn_location": part.spawn_location,
        "baseplate": part.baseplate,
        "truss": part.truss,
        "custom_appearance": part.custom_appearance,
        "textures": part.textures.iter().map(|t| json!({
            "face": enum_name(&FACES, t.face), "kind": enum_name(&SURFACES, t.kind),
        })).collect::<Vec<_>>(),
        "point_light": part.point_light.as_ref().map(point_light_view),
        "spot_light": part.spot_light.as_ref().map(spot_light_view),
        "velocity": v(&part.velocity),
        "angular_velocity": v(&part.angular_velocity),
        "group": part.group,
    })
}

pub fn attr_view(a: &AttrValue) -> Value {
    match a {
        AttrValue::Bool(b) => json!(b),
        AttrValue::F32(x) => json!(round(*x)),
        AttrValue::Vec3(xs) => json!({ "vector3": v(xs) }),
        AttrValue::Color(c) => json!({ "color": hex(*c) }),
        AttrValue::Text(s) => json!(s),
    }
}

pub fn instance_view(p: &Project, idx: usize, with_source: bool) -> Value {
    let inst = &p.instances[idx];
    let mut out = json!({
        "index": idx,
        "path": path_of(p, idx),
        "class": inst.class.to_string(),
        "name": inst.name,
        "parent": inst.parent.map(|x| path_of(p, x as usize)),
        "children": children(p, idx).into_iter().map(|c| path_of(p, c)).collect::<Vec<_>>(),
    });
    if let Some(part) = &inst.part {
        out["part"] = part_view(part);
    }
    if let Some(l) = &inst.point_light {
        out["point_light"] = point_light_view(l);
    }
    if let Some(l) = &inst.spot_light {
        out["spot_light"] = spot_light_view(l);
    }
    if let Some(s) = &inst.script {
        out["script"] = json!({ "enabled": s.enabled, "lines": s.source.lines().count() });
        if with_source {
            out["script"]["source"] = json!(s.source);
        }
    }
    if !inst.attributes.is_empty() {
        out["attributes"] = Value::Object(
            inst.attributes
                .iter()
                .map(|(k, a)| (k.clone(), attr_view(a)))
                .collect(),
        );
    }
    if inst.class == Class::Model {
        out["editor_collapsed"] = json!(inst.editor_collapsed);
    }
    out
}

pub fn lighting_view(l: &Lighting) -> Value {
    json!({
        "ambient_color": hex(l.ambient_color),
        "brightness": round(l.brightness),
        "sun_color": hex(l.sun_color),
        "sun_illuminance": round(l.sun_illuminance),
        "sun_shadows": l.sun_shadow_maps_enabled,
        "sun_rotation_degrees": v(&quat_to_euler_deg(l.sun_rotation)),
    })
}

// ---------- rotations ----------
// XYZ order like CFrame.Angles: rotate around X, then Y, then Z, intrinsic.

pub fn euler_deg_to_quat(deg: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = deg.map(|d| (d as f64).to_radians() / 2.0);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    // q = qx * qy * qz
    let q = [
        sx * cy * cz + cx * sy * sz,
        cx * sy * cz - sx * cy * sz,
        cx * cy * sz + sx * sy * cz,
        cx * cy * cz - sx * sy * sz,
    ];
    q.map(|c| c as f32)
}

pub fn quat_to_euler_deg(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q.map(|c| c as f64);
    let n = (x * x + y * y + z * z + w * w).sqrt();
    if n < 1e-9 {
        return [0.0; 3];
    }
    let (x, y, z, w) = (x / n, y / n, z / n, w / n);
    // matrix terms for R = Rx * Ry * Rz
    let m02 = 2.0 * (x * z + w * y);
    let m12 = 2.0 * (y * z - w * x);
    let m22 = 1.0 - 2.0 * (x * x + y * y);
    let m01 = 2.0 * (x * y - w * z);
    let m00 = 1.0 - 2.0 * (y * y + z * z);
    let ry = m02.clamp(-1.0, 1.0).asin();
    let (rx, rz) = if m02.abs() < 0.999_999 {
        ((-m12).atan2(m22), (-m01).atan2(m00))
    } else {
        // gimbal lock, fold everything into x
        let m21 = 2.0 * (y * z + w * x);
        let m11 = 1.0 - 2.0 * (x * x + z * z);
        (m21.atan2(m11), 0.0)
    };
    [rx, ry, rz].map(|r| r.to_degrees() as f32)
}

// ---------- edits ----------

/// Studio's own defaults for a freshly inserted part, read from a real save.
pub fn default_part(name: &str) -> Part {
    Part {
        name: name.to_owned(),
        position: [0.0, 0.5, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [4.0, 1.0, 2.0],
        color: [NEAR_WHITE, NEAR_WHITE, NEAR_WHITE, 1.0],
        material: 1,
        group: None,
        cast_shadow: true,
        anchored: true,
        can_collide: true,
        spawn_location: false,
        baseplate: false,
        custom_appearance: false,
        truss: false,
        textures: Vec::new(),
        point_light: None,
        spot_light: None,
        shape: 0,
        velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 100 {
        return Err("names must be 1 to 100 characters".into());
    }
    if name.contains('/') || name.starts_with('#') || name.chars().any(char::is_control) {
        return Err(format!(
            "{name:?} can't be used as a name here: no '/', no leading '#' and no control characters"
        ));
    }
    Ok(())
}

pub fn create(
    p: &mut Project,
    parent: usize,
    class: Class,
    name: &str,
    source: Option<String>,
) -> Result<usize> {
    if !CREATABLE.contains(&class) {
        return Err(format!(
            "creating {class} isn't supported yet: its data layout hasn't been seen in a real Studio file. \
             Supported: {}",
            CREATABLE
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    validate_name(name)?;
    if parent >= p.instances.len() {
        return Err(format!("parent #{parent} doesn't exist"));
    }
    if !class.is_script() && source.is_some() {
        return Err(format!("{class} can't hold source code"));
    }
    let script = class.is_script().then(|| Script {
        source: source.unwrap_or_else(|| default_source(class).to_owned()),
        enabled: true,
    });
    p.instances.push(Instance {
        class,
        name: name.to_owned(),
        parent: Some(parent as u64),
        part: (class == Class::Part).then(|| default_part(name)),
        point_light: None,
        spot_light: None,
        script,
        attributes: Vec::new(),
        // Studio saved its Model with this set, matching that is the safe bet
        editor_collapsed: class == Class::Model,
    });
    Ok(p.instances.len() - 1)
}

fn default_source(class: Class) -> &'static str {
    match class {
        Class::ModuleScript => "local module = {}\n\nreturn module\n",
        _ => "print(\"Hello world!\")\n",
    }
}

pub fn rename(p: &mut Project, idx: usize, name: &str) -> Result<()> {
    validate_name(name)?;
    let inst = &mut p.instances[idx];
    if inst.class.is_service() {
        return Err(format!("{} is a service and can't be renamed", inst.name));
    }
    inst.name = name.to_owned();
    // the part keeps its own copy of the name, keep them in sync
    if let Some(part) = &mut inst.part {
        part.name = name.to_owned();
    }
    Ok(())
}

pub fn set_source(p: &mut Project, idx: usize, source: &str) -> Result<()> {
    let inst = &mut p.instances[idx];
    match &mut inst.script {
        Some(s) => {
            s.source = source.to_owned();
            Ok(())
        }
        None => Err(format!("{} is a {}, not a script", inst.name, inst.class)),
    }
}

/// Moves `idx` and its subtree under `new_parent`, then to the end of the list
/// so the new parent still comes first.
pub fn reparent(p: &mut Project, idx: usize, new_parent: usize) -> Result<usize> {
    if p.instances[idx].class.is_service() {
        return Err("services can't be moved".into());
    }
    if new_parent == idx || descendants(p, idx).contains(&new_parent) {
        return Err("can't move an instance into itself or one of its descendants".into());
    }
    p.instances[idx].parent = Some(new_parent as u64);
    let mut subtree = vec![idx];
    subtree.extend(descendants(p, idx));
    Ok(move_to_end(p, &subtree)[0])
}

/// Copies an instance with everything below it. Returns the copy's index.
pub fn duplicate(p: &mut Project, idx: usize, new_parent: Option<usize>) -> Result<usize> {
    if p.instances[idx].class.is_service() {
        return Err("services can't be duplicated".into());
    }
    let mut subtree = vec![idx];
    subtree.extend(descendants(p, idx));
    let base = p.instances.len();
    let remap: BTreeMap<usize, usize> = subtree
        .iter()
        .enumerate()
        .map(|(n, &old)| (old, base + n))
        .collect();
    for &old in &subtree {
        let mut copy = p.instances[old].clone();
        copy.parent = match copy.parent {
            Some(par) if remap.contains_key(&(par as usize)) => Some(remap[&(par as usize)] as u64),
            other => other,
        };
        if let Some(part) = &mut copy.part {
            part.group = part
                .group
                .map(|g| remap.get(&(g as usize)).map_or(g, |&n| n as u64));
        }
        p.instances.push(copy);
    }
    if let Some(np) = new_parent {
        p.instances[base].parent = Some(np as u64);
    }
    Ok(base)
}

/// Deletes an instance and its subtree, returning how many went away.
pub fn delete(p: &mut Project, idx: usize) -> Result<usize> {
    if p.instances[idx].class.is_service() {
        return Err("services can't be deleted".into());
    }
    let mut doomed = vec![idx];
    doomed.extend(descendants(p, idx));
    doomed.sort_unstable();
    let keep: Vec<usize> = (0..p.instances.len())
        .filter(|i| doomed.binary_search(i).is_err())
        .collect();
    reorder(p, &keep);
    Ok(doomed.len())
}

fn move_to_end(p: &mut Project, moved: &[usize]) -> Vec<usize> {
    let mut sorted = moved.to_vec();
    sorted.sort_unstable();
    let mut order: Vec<usize> = (0..p.instances.len())
        .filter(|i| sorted.binary_search(i).is_err())
        .collect();
    let first_moved = order.len();
    order.extend(&sorted);
    reorder(p, &order);
    (first_moved..first_moved + sorted.len()).collect()
}

/// Rebuilds the list in `order` (old indices, possibly a subset) and fixes
/// every parent and group pointer. Pointers to dropped instances become None.
fn reorder(p: &mut Project, order: &[usize]) {
    let mut new_index = vec![None; p.instances.len()];
    for (new, &old) in order.iter().enumerate() {
        new_index[old] = Some(new as u64);
    }
    let remap = |x: Option<u64>| x.and_then(|old| new_index.get(old as usize).copied().flatten());
    let old = std::mem::take(&mut p.instances);
    p.instances = order
        .iter()
        .map(|&i| {
            let mut inst = old[i].clone();
            inst.parent = remap(inst.parent);
            if let Some(part) = &mut inst.part {
                part.group = remap(part.group);
            }
            inst
        })
        .collect();
}

// ---------- property patches ----------

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LightInput {
    /// Hex color like "#FFAA00".
    pub color: Option<String>,
    pub brightness: Option<f32>,
    pub range: Option<f32>,
    /// Spot lights only, cone angle in degrees.
    pub angle: Option<f32>,
    /// Spot lights only: Front, Back, Top, Bottom, Left or Right.
    pub face: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureInput {
    /// Front, Back, Top, Bottom, Left or Right.
    pub face: String,
    /// Studs or Inlets.
    pub kind: String,
}

/// Property changes. Leave out anything you don't want to touch.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Props {
    /// Part position [x, y, z] in studs.
    pub position: Option<[f32; 3]>,
    /// Part size [x, y, z] in studs, all positive.
    pub size: Option<[f32; 3]>,
    /// Part rotation in degrees [x, y, z], applied X then Y then Z like CFrame.Angles.
    pub rotation: Option<[f32; 3]>,
    /// Hex color like "#FF8800".
    pub color: Option<String>,
    /// 0 is opaque, 1 is invisible.
    pub transparency: Option<f32>,
    /// SmoothPlastic, Plastic, Wood, Metal, Grass, Ice or Paint.
    pub material: Option<String>,
    /// Block, Wedge, CornerWedge, Cylinder or Ball.
    pub shape: Option<String>,
    pub anchored: Option<bool>,
    pub can_collide: Option<bool>,
    pub cast_shadow: Option<bool>,
    /// Makes the part a spawn point.
    pub spawn_location: Option<bool>,
    pub velocity: Option<[f32; 3]>,
    pub angular_velocity: Option<[f32; 3]>,
    /// Surface textures, replaces the whole list. Empty list removes them.
    pub textures: Option<Vec<TextureInput>>,
    /// Adds or updates a point light on the part.
    pub point_light: Option<LightInput>,
    pub remove_point_light: Option<bool>,
    /// Adds or updates a spot light on the part.
    pub spot_light: Option<LightInput>,
    pub remove_spot_light: Option<bool>,
    /// Scripts only.
    pub enabled: Option<bool>,
    /// Attributes to set. Values may be booleans, numbers, strings,
    /// {"vector3": [x, y, z]} or {"color": "#RRGGBB"}.
    pub attributes: Option<BTreeMap<String, Value>>,
    pub remove_attributes: Option<Vec<String>>,
}

pub fn parse_hex(s: &str) -> Result<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{s:?} isn't a hex color like \"#FF8800\""));
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f32 / 255.0;
    Ok([c(0), c(2), c(4)])
}

fn finite(name: &str, xs: &[f32]) -> Result<()> {
    if xs.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(format!("{name} must be finite numbers"))
    }
}

fn light_color(input: Option<&String>, current: [f32; 4]) -> Result<[f32; 4]> {
    match input {
        Some(s) => {
            let [r, g, b] = parse_hex(s)?;
            Ok([r, g, b, 1.0])
        }
        None => Ok(current),
    }
}

fn apply_point_light(current: Option<PointLight>, input: &LightInput) -> Result<PointLight> {
    if input.angle.is_some() || input.face.is_some() {
        return Err("point lights have no angle or face".into());
    }
    let base = current.unwrap_or(PointLight {
        color: [1.0; 4],
        intensity: 1.0,
        range: 8.0,
    });
    let l = PointLight {
        color: light_color(input.color.as_ref(), base.color)?,
        intensity: input.brightness.unwrap_or(base.intensity),
        range: input.range.unwrap_or(base.range),
    };
    finite("light values", &[l.intensity, l.range])?;
    if l.intensity < 0.0 || l.range < 0.0 {
        return Err("light brightness and range can't be negative".into());
    }
    Ok(l)
}

fn apply_spot_light(current: Option<SpotLight>, input: &LightInput) -> Result<SpotLight> {
    let base = current.unwrap_or(SpotLight {
        color: [1.0; 4],
        intensity: 1.0,
        range: 16.0,
        angle: 90.0,
        face: 0,
    });
    let l = SpotLight {
        color: light_color(input.color.as_ref(), base.color)?,
        intensity: input.brightness.unwrap_or(base.intensity),
        range: input.range.unwrap_or(base.range),
        angle: input.angle.unwrap_or(base.angle),
        face: match &input.face {
            Some(f) => enum_id(&FACES, "face", f)?,
            None => base.face,
        },
    };
    finite("light values", &[l.intensity, l.range, l.angle])?;
    if l.intensity < 0.0 || l.range < 0.0 || !(0.0..=180.0).contains(&l.angle) {
        return Err(
            "spot light needs brightness and range >= 0 and an angle between 0 and 180".into(),
        );
    }
    Ok(l)
}

fn attr_from_json(key: &str, value: &Value) -> Result<AttrValue> {
    Ok(match value {
        Value::Bool(b) => AttrValue::Bool(*b),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN) as f32;
            finite(key, &[x])?;
            AttrValue::F32(x)
        }
        Value::String(s) => AttrValue::Text(s.clone()),
        Value::Object(o) if o.len() == 1 && o.contains_key("vector3") => {
            let xs: [f32; 3] = serde_json::from_value(o["vector3"].clone())
                .map_err(|_| format!("attribute {key}: vector3 needs [x, y, z]"))?;
            finite(key, &xs)?;
            AttrValue::Vec3(xs)
        }
        Value::Object(o) if o.len() == 1 && o.contains_key("color") => {
            let s = o["color"]
                .as_str()
                .ok_or(format!("attribute {key}: color needs a hex string"))?;
            let [r, g, b] = parse_hex(s)?;
            AttrValue::Color([r, g, b, 1.0])
        }
        _ => {
            return Err(format!(
                "attribute {key}: use a boolean, number, string, {{\"vector3\": [x,y,z]}} or {{\"color\": \"#RRGGBB\"}}"
            ));
        }
    })
}

/// Applies a patch and returns the names of the fields that were set. Nothing
/// is changed when any field is invalid.
pub fn set_props(p: &mut Project, idx: usize, props: &Props) -> Result<Vec<&'static str>> {
    let mut inst = p.instances[idx].clone();
    let mut changed = Vec::new();
    let class = inst.class;

    let needs_part = props.position.is_some()
        || props.size.is_some()
        || props.rotation.is_some()
        || props.color.is_some()
        || props.transparency.is_some()
        || props.material.is_some()
        || props.shape.is_some()
        || props.anchored.is_some()
        || props.can_collide.is_some()
        || props.cast_shadow.is_some()
        || props.spawn_location.is_some()
        || props.velocity.is_some()
        || props.angular_velocity.is_some()
        || props.textures.is_some()
        || props.point_light.is_some()
        || props.spot_light.is_some()
        || props.remove_point_light.is_some()
        || props.remove_spot_light.is_some();

    if needs_part {
        let part = inst.part.as_mut().ok_or_else(|| {
            format!(
                "{} is a {class}, only Parts have physical properties",
                inst.name
            )
        })?;
        if let Some(x) = props.position {
            finite("position", &x)?;
            part.position = x;
            changed.push("position");
        }
        if let Some(x) = props.size {
            finite("size", &x)?;
            if x.iter().any(|v| *v <= 0.0) {
                return Err("size values must be positive".into());
            }
            part.scale = x;
            changed.push("size");
        }
        if let Some(x) = props.rotation {
            finite("rotation", &x)?;
            part.rotation = euler_deg_to_quat(x);
            changed.push("rotation");
        }
        if let Some(c) = &props.color {
            let [r, g, b] = parse_hex(c)?;
            part.color = [r, g, b, part.color[3]];
            changed.push("color");
        }
        if let Some(t) = props.transparency {
            if !(0.0..=1.0).contains(&t) {
                return Err("transparency must be between 0 and 1".into());
            }
            part.color[3] = 1.0 - t;
            changed.push("transparency");
        }
        if let Some(m) = &props.material {
            part.material = enum_id(&MATERIALS, "material", m)?;
            changed.push("material");
        }
        if let Some(s) = &props.shape {
            part.shape = enum_id(&SHAPES, "shape", s)?;
            changed.push("shape");
        }
        for (val, field, name) in [
            (props.anchored, &mut part.anchored, "anchored"),
            (props.can_collide, &mut part.can_collide, "can_collide"),
            (props.cast_shadow, &mut part.cast_shadow, "cast_shadow"),
            (
                props.spawn_location,
                &mut part.spawn_location,
                "spawn_location",
            ),
        ] {
            if let Some(b) = val {
                *field = b;
                changed.push(name);
            }
        }
        if let Some(x) = props.velocity {
            finite("velocity", &x)?;
            part.velocity = x;
            changed.push("velocity");
        }
        if let Some(x) = props.angular_velocity {
            finite("angular_velocity", &x)?;
            part.angular_velocity = x;
            changed.push("angular_velocity");
        }
        if let Some(list) = &props.textures {
            part.textures = list
                .iter()
                .map(|t| {
                    Ok(Texture {
                        face: enum_id(&FACES, "face", &t.face)?,
                        kind: enum_id(&SURFACES, "texture kind", &t.kind)?,
                    })
                })
                .collect::<Result<_>>()?;
            changed.push("textures");
        }
        if let Some(l) = &props.point_light {
            part.point_light = Some(apply_point_light(part.point_light, l)?);
            changed.push("point_light");
        }
        if props.remove_point_light == Some(true) {
            part.point_light = None;
            changed.push("remove_point_light");
        }
        if let Some(l) = &props.spot_light {
            part.spot_light = Some(apply_spot_light(part.spot_light, l)?);
            changed.push("spot_light");
        }
        if props.remove_spot_light == Some(true) {
            part.spot_light = None;
            changed.push("remove_spot_light");
        }
    }

    if let Some(e) = props.enabled {
        let script = inst.script.as_mut().ok_or_else(|| {
            format!(
                "{} is a {class}, only scripts can be enabled or disabled",
                inst.name
            )
        })?;
        script.enabled = e;
        changed.push("enabled");
    }

    if let Some(attrs) = &props.attributes {
        for (key, value) in attrs {
            validate_name(key).map_err(|e| format!("attribute name: {e}"))?;
            let parsed = attr_from_json(key, value)?;
            match inst.attributes.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = parsed,
                None => inst.attributes.push((key.clone(), parsed)),
            }
        }
        changed.push("attributes");
    }
    if let Some(keys) = &props.remove_attributes {
        inst.attributes.retain(|(k, _)| !keys.contains(k));
        changed.push("remove_attributes");
    }

    if changed.is_empty() {
        return Err("nothing to change, pass at least one property".into());
    }
    p.instances[idx] = inst;
    Ok(changed)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LightingProps {
    /// Hex color like "#808080".
    pub ambient_color: Option<String>,
    pub brightness: Option<f32>,
    /// Hex color.
    pub sun_color: Option<String>,
    /// Sun strength, Studio's default is 8000.
    pub sun_illuminance: Option<f32>,
    pub sun_shadows: Option<bool>,
    /// Sun direction as rotation in degrees [x, y, z].
    pub sun_rotation: Option<[f32; 3]>,
}

pub fn set_lighting(l: &mut Lighting, props: &LightingProps) -> Result<Vec<&'static str>> {
    let mut next = *l;
    let mut changed = Vec::new();
    if let Some(c) = &props.ambient_color {
        let [r, g, b] = parse_hex(c)?;
        next.ambient_color = [r, g, b, next.ambient_color[3]];
        changed.push("ambient_color");
    }
    if let Some(b) = props.brightness {
        finite("brightness", &[b])?;
        next.brightness = b;
        changed.push("brightness");
    }
    if let Some(c) = &props.sun_color {
        let [r, g, b] = parse_hex(c)?;
        next.sun_color = [r, g, b, next.sun_color[3]];
        changed.push("sun_color");
    }
    if let Some(x) = props.sun_illuminance {
        finite("sun_illuminance", &[x])?;
        next.sun_illuminance = x;
        changed.push("sun_illuminance");
    }
    if let Some(s) = props.sun_shadows {
        next.sun_shadow_maps_enabled = s;
        changed.push("sun_shadows");
    }
    if let Some(r) = props.sun_rotation {
        finite("sun_rotation", &r)?;
        next.sun_rotation = euler_deg_to_quat(r);
        changed.push("sun_rotation");
    }
    if changed.is_empty() {
        return Err("nothing to change, pass at least one lighting property".into());
    }
    *l = next;
    Ok(changed)
}

/// What a brand new Studio project contains: the five services, a baseplate
/// and Studio's default lighting.
pub fn new_project(project_id: String) -> Project {
    let service = |class: Class| Instance {
        class,
        name: class.to_string(),
        parent: None,
        part: None,
        point_light: None,
        spot_light: None,
        script: None,
        attributes: Vec::new(),
        editor_collapsed: false,
    };
    let mut baseplate = default_part("Baseplate");
    baseplate.position = [0.0, -2.0, 0.0];
    baseplate.scale = [255.0, 4.0, 255.0];
    let grey = f32::from_bits(0x3EC8_C8C9);
    baseplate.color = [grey, grey, grey, 1.0];
    baseplate.material = 0;
    baseplate.baseplate = true;
    baseplate.textures = vec![Texture { face: 2, kind: 0 }, Texture { face: 3, kind: 1 }];
    let mut instances: Vec<Instance> = [
        Class::Workspace,
        Class::Lighting,
        Class::ReplicatedStorage,
        Class::ServerScriptService,
        Class::StarterPlayerScripts,
    ]
    .into_iter()
    .map(service)
    .collect();
    instances.push(Instance {
        name: "Baseplate".into(),
        parent: Some(0),
        part: Some(baseplate),
        ..service(Class::Part)
    });
    // exact bit patterns from a fresh Studio save, decimals would round differently
    let near_white = NEAR_WHITE;
    Project {
        version: crate::vrtx::CURRENT_VERSION,
        project_id: Some(project_id),
        instances,
        lighting: Lighting {
            ambient_color: [near_white, near_white, near_white, 1.0],
            brightness: 2000.0,
            sun_color: [near_white, near_white, near_white, 1.0],
            sun_illuminance: 8000.0,
            sun_shadow_maps_enabled: true,
            sun_rotation: [0xBEC9_F854, 0x3EE0_DA9C, 0x3E64_B4FD, 0x3F46_9143].map(f32::from_bits),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vrtx;

    const SHOWCASE: &[u8] = include_bytes!("../tests/fixtures/showcase.vrtx");

    fn showcase() -> Project {
        vrtx::decode(SHOWCASE).unwrap()
    }

    fn parents_come_first(p: &Project) -> bool {
        p.instances
            .iter()
            .enumerate()
            .all(|(i, inst)| inst.parent.is_none_or(|par| (par as usize) < i))
    }

    #[test]
    fn fresh_project_matches_studio_template() {
        // the showcase started as a new project, so its first six instances and
        // lighting are exactly what Studio generates
        let studio = showcase();
        let ours = new_project(studio.project_id.clone().unwrap());
        assert_eq!(ours.instances[..], studio.instances[..6]);
        assert_eq!(ours.lighting, studio.lighting);
    }

    #[test]
    fn default_part_matches_a_studio_inserted_part() {
        let p = showcase();
        let studio_part = p.instances[8].part.clone().unwrap();
        let mut ours = default_part(&studio_part.name);
        ours.position = studio_part.position;
        ours.scale = studio_part.scale;
        ours.shape = studio_part.shape;
        assert_eq!(ours, studio_part);
    }

    #[test]
    fn resolve_paths_and_indices() {
        let p = showcase();
        assert_eq!(resolve(&p, "Workspace/Baseplate"), Ok(5));
        assert_eq!(resolve(&p, "#5"), Ok(5));
        assert_eq!(resolve(&p, "/ServerScriptService/Script/"), Ok(21));
        let err = resolve(&p, "Workspace/Part").unwrap_err();
        assert!(err.contains("#8"), "{err}");
        assert!(resolve(&p, "#999").is_err());
        assert!(resolve(&p, "Nope/Thing").is_err());
    }

    #[test]
    fn create_rename_and_set_props() {
        let mut p = showcase();
        let ws = resolve(&p, "Workspace").unwrap();
        let idx = create(&mut p, ws, Class::Part, "Lava", None).unwrap();
        rename(&mut p, idx, "HotLava").unwrap();
        assert_eq!(p.instances[idx].part.as_ref().unwrap().name, "HotLava");
        let props = Props {
            color: Some("#FF4400".into()),
            transparency: Some(0.25),
            material: Some("Enum.Material.Metal".into()),
            rotation: Some([0.0, 90.0, 0.0]),
            point_light: Some(LightInput {
                brightness: Some(3.0),
                ..Default::default()
            }),
            attributes: Some(BTreeMap::from([("Damage".into(), json!(25))])),
            ..Default::default()
        };
        set_props(&mut p, idx, &props).unwrap();
        let part = p.instances[idx].part.as_ref().unwrap();
        assert_eq!(hex(part.color), "#FF4400");
        assert!((part.color[3] - 0.75).abs() < 1e-6);
        assert_eq!(part.material, 3);
        assert_eq!(part.point_light.unwrap().intensity, 3.0);
        let back = vrtx::decode(&vrtx::encode(&p).unwrap()).unwrap();
        assert_eq!(back.instances, p.instances);
    }

    #[test]
    fn invalid_patch_changes_nothing() {
        let mut p = showcase();
        let before = p.clone();
        let props = Props {
            color: Some("#00FF00".into()),
            size: Some([1.0, -1.0, 1.0]),
            ..Default::default()
        };
        assert!(set_props(&mut p, 5, &props).is_err());
        assert_eq!(p, before);
        let script_only = Props {
            enabled: Some(false),
            ..Default::default()
        };
        assert!(set_props(&mut p, 5, &script_only).is_err());
    }

    #[test]
    fn unsupported_classes_are_refused() {
        let mut p = showcase();
        assert!(create(&mut p, 0, Class::IntValue, "Score", None).is_err());
        assert!(create(&mut p, 0, Class::Part, "bad/name", None).is_err());
        assert!(create(&mut p, 0, Class::Part, "Code", Some("print(1)".into())).is_err());
    }

    #[test]
    fn delete_remaps_parents() {
        let mut p = showcase();
        let model = resolve(&p, "Workspace/Model").unwrap();
        let script_path = path_of(&p, 21);
        let removed = delete(&mut p, model).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(p.instances.len(), 20);
        assert!(resolve(&p, &script_path).is_ok());
        assert!(parents_come_first(&p));
        assert!(delete(&mut p, 0).is_err());
    }

    #[test]
    fn reparent_and_duplicate_keep_parents_first() {
        let mut p = showcase();
        let rs = resolve(&p, "ReplicatedStorage").unwrap();
        let model = resolve(&p, "Workspace/Model").unwrap();
        let moved = reparent(&mut p, model, rs).unwrap();
        assert_eq!(path_of(&p, moved), "ReplicatedStorage/Model");
        assert_eq!(children(&p, moved).len(), 1);
        assert!(parents_come_first(&p));

        let copy = duplicate(&mut p, moved, None).unwrap();
        assert_eq!(children(&p, copy).len(), 1);
        assert_eq!(p.instances[copy].parent, Some(rs as u64));
        assert!(parents_come_first(&p));

        let child = children(&p, copy)[0];
        assert!(reparent(&mut p, copy, child).is_err());
        assert!(reparent(&mut p, 0, 2).is_err());
    }

    #[test]
    fn euler_roundtrip() {
        for deg in [
            [0.0, 0.0, 0.0],
            [30.0, 45.0, 60.0],
            [-90.0, 10.0, 170.0],
            [0.0, 90.0, 0.0],
        ] {
            let back = quat_to_euler_deg(euler_deg_to_quat(deg));
            for i in 0..3 {
                assert!((back[i] - deg[i]).abs() < 0.01, "{deg:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn lighting_patch() {
        let mut p = showcase();
        set_lighting(
            &mut p.lighting,
            &LightingProps {
                sun_color: Some("#FFCC88".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(hex(p.lighting.sun_color), "#FFCC88");
        assert!(set_lighting(&mut p.lighting, &LightingProps::default()).is_err());
    }
}
