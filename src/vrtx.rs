//! Reader and writer for Vortex Studio's `.vrtx` project files.
//!
//! On disk it's `VRTX`, a format version byte, then a zstd frame holding a
//! bincode 1 payload (fixed width little endian ints, u64 lengths, u32 enum
//! tags, one byte option and bool tags). Field names and order come from the
//! serde metadata Vortex ships in its binaries, see `docs/FORMAT.md`.

use std::fmt;

use serde::Serialize;

pub const MAGIC: &[u8; 4] = b"VRTX";
/// The version Studio writes today. We always write this one.
pub const CURRENT_VERSION: u8 = 5;
const OLDEST_VERSION: u8 = 4;
const MAX_STRING: u64 = 64 << 20;
const MAX_COUNT: u64 = 1 << 24;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Project {
    /// Format version the file was read from.
    pub version: u8,
    pub project_id: Option<String>,
    pub instances: Vec<Instance>,
    pub lighting: Lighting,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Instance {
    pub class: Class,
    pub name: String,
    /// Index into `Project::instances`. Services have none.
    pub parent: Option<u64>,
    pub part: Option<Part>,
    pub point_light: Option<PointLight>,
    pub spot_light: Option<SpotLight>,
    pub script: Option<Script>,
    pub attributes: Vec<(String, AttrValue)>,
    pub editor_collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Part {
    pub name: String,
    pub position: [f32; 3],
    /// Quaternion as x, y, z, w.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// RGBA, alpha is opacity (1 minus Transparency).
    pub color: [f32; 4],
    pub material: u32,
    pub group: Option<u64>,
    pub cast_shadow: bool,
    pub anchored: bool,
    pub can_collide: bool,
    pub spawn_location: bool,
    pub baseplate: bool,
    pub custom_appearance: bool,
    pub truss: bool,
    pub textures: Vec<Texture>,
    pub point_light: Option<PointLight>,
    pub spot_light: Option<SpotLight>,
    // the three below arrived with format 5, v4 files read them as defaults
    pub shape: u32,
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Texture {
    pub face: u32,
    pub kind: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PointLight {
    pub color: [f32; 4],
    pub intensity: f32,
    pub range: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SpotLight {
    pub color: [f32; 4],
    pub intensity: f32,
    pub range: f32,
    pub angle: f32,
    pub face: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Script {
    pub source: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "value")]
pub enum AttrValue {
    Bool(bool),
    F32(f32),
    Vec3([f32; 3]),
    Color([f32; 4]),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Lighting {
    pub ambient_color: [f32; 4],
    pub brightness: f32,
    pub sun_color: [f32; 4],
    pub sun_illuminance: f32,
    pub sun_shadow_maps_enabled: bool,
    pub sun_rotation: [f32; 4],
}

macro_rules! classes {
    ($($name:ident = $tag:literal),* $(,)?) => {
        /// Instance classes, numbered the way the file stores them.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        pub enum Class {
            $($name,)*
            Unknown(u32),
        }

        impl Class {
            pub const ALL: &[Class] = &[$(Class::$name),*];

            pub fn from_tag(tag: u32) -> Self {
                match tag {
                    $($tag => Class::$name,)*
                    other => Class::Unknown(other),
                }
            }

            pub fn tag(self) -> u32 {
                match self {
                    $(Class::$name => $tag,)*
                    Class::Unknown(t) => t,
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $(stringify!($name) => Some(Class::$name),)*
                    _ => None,
                }
            }
        }
    };
}

classes! {
    Workspace = 0, Lighting = 1, Part = 2, Model = 3, Folder = 4, PointLight = 5,
    SpotLight = 6, LocalScript = 7, Script = 8, ModuleScript = 9, ReplicatedStorage = 10,
    StarterPlayerScripts = 11, ServerScriptService = 12, RemoteEvent = 13, BindableEvent = 14,
    RemoteFunction = 15, BindableFunction = 16, IntValue = 17, StringValue = 18,
    BodyVelocity = 19, BodyPosition = 20, BodyAngularVelocity = 21, VectorForce = 22, Torque = 23,
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Class::Unknown(t) => write!(f, "Unknown({t})"),
            known => fmt::Debug::fmt(known, f),
        }
    }
}

impl Class {
    pub fn is_service(self) -> bool {
        matches!(
            self,
            Class::Workspace
                | Class::Lighting
                | Class::ReplicatedStorage
                | Class::StarterPlayerScripts
                | Class::ServerScriptService
        )
    }

    pub fn is_script(self) -> bool {
        matches!(
            self,
            Class::Script | Class::LocalScript | Class::ModuleScript
        )
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    NotVrtx,
    UnsupportedVersion(u8),
    Decompress(String),
    Truncated { at: usize, wanted: &'static str },
    Invalid { at: usize, what: String },
    TrailingBytes(usize),
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotVrtx => write!(f, "not a .vrtx file (missing VRTX header)"),
            Error::UnsupportedVersion(v) => write!(
                f,
                "unsupported .vrtx format version {v}, this build understands {OLDEST_VERSION} to {CURRENT_VERSION}"
            ),
            Error::Decompress(e) => write!(f, "couldn't decompress project data: {e}"),
            Error::Truncated { at, wanted } => {
                write!(
                    f,
                    "project data ends early at byte {at} while reading {wanted}"
                )
            }
            Error::Invalid { at, what } => write!(f, "invalid project data at byte {at}: {what}"),
            Error::TrailingBytes(n) => write!(f, "{n} unexpected bytes after the project data"),
            Error::Unsupported(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// Parses a whole `.vrtx` file.
pub fn decode(file: &[u8]) -> Result<Project> {
    let (version, payload) = unwrap_container(file)?;
    decode_payload(version, &payload)
}

/// Serializes as the current format version, compressed and ready to write.
pub fn encode(project: &Project) -> Result<Vec<u8>> {
    let payload = encode_payload(project);
    let compressed =
        zstd::encode_all(&payload[..], 3).map_err(|e| Error::Decompress(e.to_string()))?;
    let mut out = Vec::with_capacity(5 + compressed.len());
    out.extend_from_slice(MAGIC);
    out.push(CURRENT_VERSION);
    out.extend_from_slice(&compressed);
    Ok(out)
}

pub fn unwrap_container(file: &[u8]) -> Result<(u8, Vec<u8>)> {
    if file.len() < 5 || &file[..4] != MAGIC {
        return Err(Error::NotVrtx);
    }
    let version = file[4];
    if !(OLDEST_VERSION..=CURRENT_VERSION).contains(&version) {
        return Err(Error::UnsupportedVersion(version));
    }
    // a corrupted file shouldn't be able to make us allocate gigabytes
    let mut payload = Vec::new();
    let decoder = zstd::Decoder::new(&file[5..]).map_err(|e| Error::Decompress(e.to_string()))?;
    std::io::Read::read_to_end(&mut std::io::Read::take(decoder, 512 << 20), &mut payload)
        .map_err(|e| Error::Decompress(e.to_string()))?;
    Ok((version, payload))
}

pub fn decode_payload(version: u8, payload: &[u8]) -> Result<Project> {
    let mut r = Reader {
        buf: payload,
        pos: 0,
    };
    let project_id = r.option(|r| r.string())?;
    let count = r.len("instance count")?;
    let mut instances = Vec::with_capacity(count.min(4096) as usize);
    for _ in 0..count {
        instances.push(read_instance(&mut r, version)?);
    }
    let lighting = Lighting {
        ambient_color: r.f32s()?,
        brightness: r.f32()?,
        sun_color: r.f32s()?,
        sun_illuminance: r.f32()?,
        sun_shadow_maps_enabled: r.bool()?,
        sun_rotation: r.f32s()?,
    };
    if r.pos != payload.len() {
        return Err(Error::TrailingBytes(payload.len() - r.pos));
    }
    Ok(Project {
        version,
        project_id,
        instances,
        lighting,
    })
}

pub fn encode_payload(p: &Project) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(4096));
    w.option(&p.project_id, |w, s| w.string(s));
    w.u64(p.instances.len() as u64);
    for inst in &p.instances {
        write_instance(&mut w, inst);
    }
    let l = &p.lighting;
    w.f32s(&l.ambient_color);
    w.f32(l.brightness);
    w.f32s(&l.sun_color);
    w.f32(l.sun_illuminance);
    w.bool(l.sun_shadow_maps_enabled);
    w.f32s(&l.sun_rotation);
    w.0
}

fn read_instance(r: &mut Reader, version: u8) -> Result<Instance> {
    Ok(Instance {
        class: Class::from_tag(r.u32()?),
        name: r.string()?,
        parent: r.option(|r| r.u64())?,
        part: r.option(|r| read_part(r, version))?,
        point_light: r.option(read_point_light)?,
        spot_light: r.option(read_spot_light)?,
        script: r.option(|r| {
            Ok(Script {
                source: r.string()?,
                enabled: r.bool()?,
            })
        })?,
        attributes: {
            let n = r.len("attribute count")?;
            let mut attrs = Vec::with_capacity(n.min(256) as usize);
            for _ in 0..n {
                attrs.push((r.string()?, read_attr(r)?));
            }
            attrs
        },
        editor_collapsed: r.bool()?,
    })
}

fn write_instance(w: &mut Writer, i: &Instance) {
    w.u32(i.class.tag());
    w.string(&i.name);
    w.option(&i.parent, |w, p| w.u64(*p));
    w.option(&i.part, write_part);
    w.option(&i.point_light, write_point_light);
    w.option(&i.spot_light, write_spot_light);
    w.option(&i.script, |w, s| {
        w.string(&s.source);
        w.bool(s.enabled);
    });
    w.u64(i.attributes.len() as u64);
    for (key, value) in &i.attributes {
        w.string(key);
        write_attr(w, value);
    }
    w.bool(i.editor_collapsed);
}

fn read_part(r: &mut Reader, version: u8) -> Result<Part> {
    let mut part = Part {
        name: r.string()?,
        position: r.f32s()?,
        rotation: r.f32s()?,
        scale: r.f32s()?,
        color: r.f32s()?,
        material: r.u32()?,
        group: r.option(|r| r.u64())?,
        cast_shadow: r.bool()?,
        anchored: r.bool()?,
        can_collide: r.bool()?,
        spawn_location: r.bool()?,
        baseplate: r.bool()?,
        custom_appearance: r.bool()?,
        truss: r.bool()?,
        textures: {
            let n = r.len("texture count")?;
            let mut t = Vec::with_capacity(n.min(64) as usize);
            for _ in 0..n {
                t.push(Texture {
                    face: r.u32()?,
                    kind: r.u32()?,
                });
            }
            t
        },
        point_light: r.option(read_point_light)?,
        spot_light: r.option(read_spot_light)?,
        shape: 0,
        velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
    };
    if version >= 5 {
        part.shape = r.u32()?;
        part.velocity = r.f32s()?;
        part.angular_velocity = r.f32s()?;
    }
    Ok(part)
}

fn write_part(w: &mut Writer, p: &Part) {
    w.string(&p.name);
    w.f32s(&p.position);
    w.f32s(&p.rotation);
    w.f32s(&p.scale);
    w.f32s(&p.color);
    w.u32(p.material);
    w.option(&p.group, |w, g| w.u64(*g));
    for flag in [
        p.cast_shadow,
        p.anchored,
        p.can_collide,
        p.spawn_location,
        p.baseplate,
        p.custom_appearance,
        p.truss,
    ] {
        w.bool(flag);
    }
    w.u64(p.textures.len() as u64);
    for t in &p.textures {
        w.u32(t.face);
        w.u32(t.kind);
    }
    w.option(&p.point_light, write_point_light);
    w.option(&p.spot_light, write_spot_light);
    w.u32(p.shape);
    w.f32s(&p.velocity);
    w.f32s(&p.angular_velocity);
}

fn read_point_light(r: &mut Reader) -> Result<PointLight> {
    Ok(PointLight {
        color: r.f32s()?,
        intensity: r.f32()?,
        range: r.f32()?,
    })
}

fn write_point_light(w: &mut Writer, l: &PointLight) {
    w.f32s(&l.color);
    w.f32(l.intensity);
    w.f32(l.range);
}

fn read_spot_light(r: &mut Reader) -> Result<SpotLight> {
    Ok(SpotLight {
        color: r.f32s()?,
        intensity: r.f32()?,
        range: r.f32()?,
        angle: r.f32()?,
        face: r.u32()?,
    })
}

fn write_spot_light(w: &mut Writer, l: &SpotLight) {
    w.f32s(&l.color);
    w.f32(l.intensity);
    w.f32(l.range);
    w.f32(l.angle);
    w.u32(l.face);
}

fn read_attr(r: &mut Reader) -> Result<AttrValue> {
    let at = r.pos;
    Ok(match r.u32()? {
        0 => AttrValue::Bool(r.bool()?),
        1 => AttrValue::F32(r.f32()?),
        2 => AttrValue::Vec3(r.f32s()?),
        3 => AttrValue::Color(r.f32s()?),
        4 => AttrValue::Text(r.string()?),
        // Enum's payload layout isn't known yet, bailing beats guessing
        5 => {
            return Err(Error::Unsupported(format!(
                "attribute at byte {at} holds an Enum value, which this version can't read yet"
            )));
        }
        tag => {
            return Err(Error::Invalid {
                at,
                what: format!("unknown attribute type {tag}"),
            });
        }
    })
}

fn write_attr(w: &mut Writer, v: &AttrValue) {
    match v {
        AttrValue::Bool(b) => {
            w.u32(0);
            w.bool(*b);
        }
        AttrValue::F32(x) => {
            w.u32(1);
            w.f32(*x);
        }
        AttrValue::Vec3(v) => {
            w.u32(2);
            w.f32s(v);
        }
        AttrValue::Color(c) => {
            w.u32(3);
            w.f32s(c);
        }
        AttrValue::Text(s) => {
            w.u32(4);
            w.string(s);
        }
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize, wanted: &'static str) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or(Error::Truncated {
                at: self.pos,
                wanted,
            })?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1, "byte")?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4, "u32")?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8, "u64")?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4, "f32")?.try_into().unwrap()))
    }

    fn f32s<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }

    fn bool(&mut self) -> Result<bool> {
        let at = self.pos;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(Error::Invalid {
                at,
                what: format!("expected a bool, found {b:#04x}"),
            }),
        }
    }

    fn len(&mut self, what: &'static str) -> Result<u64> {
        let at = self.pos;
        let n = self.u64()?;
        if n > MAX_COUNT {
            return Err(Error::Invalid {
                at,
                what: format!("{what} of {n} is not plausible"),
            });
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String> {
        let at = self.pos;
        let n = self.u64()?;
        if n > MAX_STRING {
            return Err(Error::Invalid {
                at,
                what: format!("string length {n} is not plausible"),
            });
        }
        let bytes = self.take(n as usize, "string")?;
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::Invalid {
            at,
            what: "string is not valid UTF-8".into(),
        })
    }

    fn option<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<Option<T>> {
        let at = self.pos;
        match self.u8()? {
            0 => Ok(None),
            1 => f(self).map(Some),
            b => Err(Error::Invalid {
                at,
                what: format!("expected an option tag, found {b:#04x}"),
            }),
        }
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.f32(*x);
        }
    }

    fn bool(&mut self, v: bool) {
        self.0.push(v as u8);
    }

    fn string(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.0.extend_from_slice(s.as_bytes());
    }

    fn option<T>(&mut self, v: &Option<T>, f: impl FnOnce(&mut Self, &T)) {
        match v {
            None => self.0.push(0),
            Some(x) => {
                self.0.push(1);
                f(self, x);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWCASE: &[u8] = include_bytes!("../tests/fixtures/showcase.vrtx");

    // services plus the baseplate, which is what a fresh Studio project holds
    fn minimal() -> Project {
        let mut p = decode(SHOWCASE).unwrap();
        p.instances.truncate(6);
        p
    }

    fn roundtrips(file: &[u8]) {
        let (version, payload) = unwrap_container(file).unwrap();
        let project = decode_payload(version, &payload).unwrap();
        assert_eq!(
            encode_payload(&project),
            payload,
            "payload changed on re-encode"
        );
        let again = decode(&encode(&project).unwrap()).unwrap();
        assert_eq!(again.instances, project.instances);
    }

    #[test]
    fn fixtures_roundtrip_byte_for_byte() {
        roundtrips(SHOWCASE);
        roundtrips(&encode(&minimal()).unwrap());
    }

    #[test]
    fn showcase_contents() {
        let p = decode(SHOWCASE).unwrap();
        assert_eq!(p.version, 5);
        assert_eq!(p.instances.len(), 22);
        let base = &p.instances[5];
        assert_eq!(base.class, Class::Part);
        let part = base.part.as_ref().unwrap();
        assert!(part.baseplate);
        assert_eq!(part.position, [0.0, -2.0, 0.0]);
        assert_eq!(
            part.textures,
            [Texture { face: 2, kind: 0 }, Texture { face: 3, kind: 1 }]
        );
        let script = p
            .instances
            .iter()
            .find(|i| i.class == Class::Script)
            .unwrap();
        assert_eq!(
            script.script.as_ref().unwrap().source,
            "local test = \"Test\";\nprint(test);"
        );
        assert!(
            p.instances
                .iter()
                .any(|i| i.class == Class::Model && i.editor_collapsed)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(decode(b"nope"), Err(Error::NotVrtx));
        assert_eq!(decode(b"VRTX\x09abc"), Err(Error::UnsupportedVersion(9)));
        assert!(matches!(
            decode(b"VRTX\x05not zstd"),
            Err(Error::Decompress(_))
        ));
    }

    #[test]
    fn truncated_payload_is_an_error_not_a_panic() {
        let (_, payload) = unwrap_container(SHOWCASE).unwrap();
        for cut in [0, 1, 9, 40, 300, payload.len() - 1] {
            assert!(decode_payload(5, &payload[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn attributes_and_lights_roundtrip() {
        let mut p = minimal();
        let inst = &mut p.instances[5];
        inst.attributes = vec![
            ("Speed".into(), AttrValue::F32(16.0)),
            ("Tag".into(), AttrValue::Text("enemy".into())),
            ("On".into(), AttrValue::Bool(true)),
            ("Dir".into(), AttrValue::Vec3([0.0, 1.0, 0.0])),
            ("Tint".into(), AttrValue::Color([1.0, 0.0, 0.0, 1.0])),
        ];
        inst.part.as_mut().unwrap().point_light = Some(PointLight {
            color: [1.0, 0.9, 0.8, 1.0],
            intensity: 2.0,
            range: 12.0,
        });
        let back = decode(&encode(&p).unwrap()).unwrap();
        assert_eq!(back.instances, p.instances);
    }

    #[test]
    fn class_tags_are_stable() {
        for &c in Class::ALL {
            assert_eq!(Class::from_tag(c.tag()), c);
        }
        assert_eq!(Class::RemoteEvent.tag(), 13);
        assert_eq!(Class::from_tag(99), Class::Unknown(99));
    }
}
