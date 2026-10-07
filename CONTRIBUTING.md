# Contributing

Thanks for helping. Bug reports, sample files, API corrections and pull requests are all welcome.

## Reporting problems

Open an issue with the tool name, the arguments you (or your assistant) passed and the error text. If a project fails to load, attach the `.vrtx` if you can share it, or at least say which Studio version saved it.

## Sample files

The most valuable thing right now is a `.vrtx` that uses something we haven't seen in a real file: PointLight or SpotLight instances, IntValue, StringValue, Folder, BodyVelocity, BodyPosition, BodyAngularVelocity, VectorForce, Torque, or attributes set in Studio. Check that it decodes with

```sh
VRTX_SAMPLES=/folder/with/files cargo test --test samples -- --nocapture
```

and attach it to an issue. We only add files to `tests/fixtures` when the author agrees.

## API corrections

`data/api.json` is the single source for the API tools, the linter and the documentation site. When you change it:

* Only add what you've confirmed in Vortex, ideally by running a script. If you're going by Roblox docs or an editor label, set `"unverified": true`.
* Keep descriptions short and practical.
* Run the tests, one of them checks the file is consistent.

## Development

You need Rust 1.89 or newer.

```sh
cargo build
cargo test
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

CI runs the last three on Linux, Windows and macOS.

To try the server by hand, point any MCP client at `target/debug/vortexstudio-mcp`, or use the [MCP Inspector](https://github.com/modelcontextprotocol/inspector):

```sh
npx @modelcontextprotocol/inspector target/debug/vortexstudio-mcp
```

## Layout

| File | Purpose |
| --- | --- |
| `src/vrtx.rs` | The file format, byte for byte |
| `src/scene.rs` | Paths, property views and every edit |
| `src/store.rs` | Loading, atomic saves, backups |
| `src/sync.rs` | Script export and import |
| `src/check.rs` | Project health check |
| `src/lint.rs` | Vortex aware Luau lint |
| `src/api.rs` | Queries over `data/api.json` |
| `src/server.rs` | MCP tools |

## Guidelines

* The format code must keep round tripping real files exactly. Never write a field we haven't seen in a real file; refuse with a clear message instead.
* Edits validate everything before touching the project, a failed tool call leaves the file untouched.
* Tool errors are read by language models: say what was wrong and what to do instead.
* Comments explain why. Update the README and `docs/FORMAT.md` when behavior or the format changes.

## Commits and releases

Short imperative titles in sentence case, no trailing period, for example `Support Folder instances`.

To release, bump the version in `Cargo.toml` and push a matching tag like `v1.1.0`. The release workflow builds Linux, Windows and macOS binaries and publishes them with checksums.
