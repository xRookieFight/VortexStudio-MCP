# The `.vrtx` format

Vortex Studio saves projects as `.vrtx`. Nothing about the format is published, so this is what we worked out from real files and from the serde metadata both Vortex binaries carry (field and variant names, struct sizes). `src/vrtx.rs` implements exactly what's written here.

## Container

| Bytes | Meaning |
| --- | --- |
| 0..4 | `VRTX` |
| 4 | format version, `4` or `5` (Studio 0.6.1 writes 5) |
| 5.. | one zstd frame |

The decompressed payload is [bincode 1](https://github.com/bincode-org/bincode) with its default settings: little endian, fixed width integers, `u64` lengths for strings and sequences, a `u32` tag for enum variants, one byte (`0` or `1`) for options and booleans. Floats are `f32`.

## Payload (`ProjectDataV2`)

```
project_id : Option<String>        32 lowercase hex chars in practice
instances  : Vec<InstanceData>
lighting   : LightingData
```

### InstanceData

```
class_name       : u32            see the class table
name             : String
parent           : Option<u64>    index into instances, None for services
part             : Option<PartData>
point_light      : Option<PointLightData>
spot_light       : Option<SpotLightData>
script           : Option<ScriptData>
attributes       : Vec<(String, PropertyValueData)>
editor_collapsed : bool           explorer state, Studio saves Models with true
```

Studio always lists a parent before its children. Our edits keep it that way.

### PartData

```
name              : String       duplicate of the instance name
position          : [f32; 3]
rotation          : [f32; 4]     quaternion x, y, z, w
scale             : [f32; 3]     the Size property
color             : [f32; 4]     RGBA, alpha = 1 - Transparency
material          : u32          MaterialKindData
group             : Option<u64>  unused in every file seen, kept as is
cast_shadow       : bool
anchored          : bool
can_collide       : bool
spawn_location    : bool
baseplate         : bool
custom_appearance : bool
truss             : bool
textures          : Vec<TextureData { face: u32, kind: u32 }>
point_light       : Option<PointLightData>
spot_light        : Option<SpotLightData>
shape             : u32          version 5 only, PartShapeData
velocity          : [f32; 3]     version 5 only
angular_velocity  : [f32; 3]     version 5 only
```

Version 4 files simply stop after `spot_light`. We read them with `shape = Block` and zero velocities and always write version 5.

```
PointLightData { color: [f32; 4], intensity: f32, range: f32 }
SpotLightData  { color: [f32; 4], intensity: f32, range: f32, angle: f32, face: u32 }
ScriptData     { source: String, enabled: bool }
LightingData   {
    ambient_color: [f32; 4], brightness: f32,
    sun_color: [f32; 4], sun_illuminance: f32,
    sun_shadow_maps_enabled: bool, sun_rotation: [f32; 4],
}
```

### PropertyValueData (attribute values)

| Tag | Variant | Payload |
| --- | --- | --- |
| 0 | Bool | `bool` |
| 1 | F32 | `f32` |
| 2 | Vec3 | `[f32; 3]` |
| 3 | Color | `[f32; 4]` |
| 4 | Text | `String` |
| 5 | Enum | unknown, reading one is reported as unsupported |

### Enums

| Enum | Values in order |
| --- | --- |
| class (`ClassNameData`) | Workspace, Lighting, Part, Model, Folder, PointLight, SpotLight, LocalScript, Script, ModuleScript, ReplicatedStorage, StarterPlayerScripts, ServerScriptService, RemoteEvent, BindableEvent, RemoteFunction, BindableFunction, IntValue, StringValue, BodyVelocity, BodyPosition, BodyAngularVelocity, VectorForce, Torque |
| material | SmoothPlastic, Plastic, Wood, Metal, Grass, Ice, Paint |
| shape | Block, Wedge, CornerWedge, Cylinder, Ball |
| face | Front, Back, Top, Bottom, Left, Right |
| texture kind | Studs, Inlets |

## How sure are we?

* **Verified by round trip.** Every file we could find (Studio saves and published community projects, versions 4 and 5) decodes completely and, for version 5, re-encodes to the exact same bytes. The test suite does this for the fixture in `tests/fixtures`, and `VRTX_SAMPLES=dir cargo test --test samples` does it for any folder of files.
* **Verified against Studio output.** A fresh project and a freshly inserted part built by this crate are byte identical to what Studio produces.
* **Verified in Studio.** A project written entirely by this crate opens in Studio 0.6.1 as intended: parts in every material we tested (Wood, Ice, Metal, Grass), a Ball and a Wedge, transparency, rotations, a point light on a part, a Model with children, attributes, a RemoteEvent, a Script and changed lighting.
* **Inferred from names.** The instance level `point_light` and `spot_light` slots and the spot light struct come from serde metadata but haven't been checked against Studio yet. Class ids 17 to 23 (IntValue onwards) only exist in Studio and their data hasn't been seen either. That's why the MCP refuses to create those classes for now.
* **Material order.** The binaries list seven materials but only six names are visible because `Plastic` shares bytes with `SmoothPlastic`. The order above matches the Luau `Enum.Material` table and Studio's default (new parts are 1, `Plastic`), and Studio shows the materials we write correctly.

If you have a project that uses lights, values, body movers or attributes and the tools reject it, please open an issue with the file attached. One sample is enough to pin those parts down.
