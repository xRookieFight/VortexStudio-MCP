# VortexStudio-MCP

An [MCP](https://modelcontextprotocol.io) server that lets AI assistants work on [Vortex](https://playvortex.io) games. It reads and edits Vortex Studio projects (`.vrtx`), knows which parts of the Roblox style Luau API Vortex actually has, and lints scripts for the mistakes models tend to make when they assume full Roblox.

> Unofficial community project, not affiliated with or endorsed by Vortex.

## What it can do

**Understand a project**

* `project_info`, `list_instances`, `get_instance`, `find_instances` show the instance tree and every property: transform, color, material, shape, physics flags, lights, textures, attributes.
* `read_script` and `search_scripts` read and grep all Luau code.
* `check_project` reports broken structure, scripts in places where they never run, duplicate sibling names and a Vortex lint of every script.

**Write correct Luau**

* `api_overview`, `api_search` and `api_get` cover the services, classes, datatypes, enums, globals, libraries and engine limits Vortex exposes, collected from the engine's own script bindings.
* `lint_luau` catches syntax errors, Roblox only services and classes (`DataStoreService`, `Sound`, ...), unknown enum items and datatype members (`Enum.Material.Neon`, `Color3.fromHSV`), and client or server only APIs used on the wrong side (`LocalPlayer` in a server Script).

**Edit a project**

* `write_script`, `create_instance`, `set_properties`, `rename_instance`, `move_instance`, `duplicate_instance`, `delete_instance`, `set_lighting`, `new_project`.
* `export_scripts` and `import_scripts` move all scripts to a folder of `.luau` files and back, so you can edit them in your editor and keep them in git.
* `list_backups` and `restore_backup` undo anything.

Every edit backs the project up into `.vrtx-backups/` next to it (the last 20 are kept, names use UTC timestamps), writes atomically and reads the new file back before replacing the old one. Scripts with syntax errors are refused unless you explicitly allow them.

## Install

Download the binary for your system from [Releases](https://github.com/xRookieFight/VortexStudio-MCP/releases), or build it with Rust 1.89+:

```sh
cargo install --git https://github.com/xRookieFight/VortexStudio-MCP
```

Then register it with your MCP client. The server speaks MCP over stdio and takes no arguments.

**Claude Code**

```sh
claude mcp add vortex-studio -- /path/to/vortexstudio-mcp
```

**Claude Desktop, Cursor, Windsurf and most others** use the same JSON shape in their MCP settings:

```json
{
  "mcpServers": {
    "vortex-studio": {
      "command": "/path/to/vortexstudio-mcp"
    }
  }
}
```

On Windows the command is the full path to `vortexstudio-mcp.exe`.

## Using it

Ask your assistant things like:

* "Open `~/Games/obby.vrtx` and tell me what's in it."
* "Add a kill brick that resets players who touch it, and the server script for it."
* "Why doesn't my LocalScript run?"
* "Export the scripts to `./src` so I can edit them in VS Code." Later: "Import them back."

Instances are addressed by path from a service, like `Workspace/Tower/Floor1`, or by index like `#12` when names repeat. File paths can be absolute, start with `~`, or use Wine style `Z:\home\me\game.vrtx` when you run Studio through Wine.

### Studio and edits

Studio keeps the project in memory. If it's open while the MCP edits the file:

1. Reopen the project in Studio to see the changes.
2. Don't save from the old Studio window before that, it would overwrite the edits.

The tools warn when they see Studio running.

## Limits

* Creating PointLight, SpotLight, Folder, IntValue, StringValue and the body movers isn't supported yet, because no real file using them has been seen and guessing their layout could produce projects Studio rejects. Parts can still get point and spot lights through `set_properties`. Existing projects that contain those classes are read and saved without losing anything.
* The API reference marks entries that come from Roblox conventions rather than Vortex's bindings as `unverified`.
* Publishing isn't supported.

The file format is documented in [docs/FORMAT.md](docs/FORMAT.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Sample `.vrtx` files that use lights, values, body movers or attributes are the most useful contribution right now.

## License

[MIT](LICENSE). Vortex and Vortex Studio are proprietary software owned by their developers.
