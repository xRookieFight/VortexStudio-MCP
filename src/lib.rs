//! Vortex Studio tooling behind the MCP server: the `.vrtx` format, a model of
//! the scene tree, edits, the Luau API database and a Vortex aware linter.

pub mod api;
pub mod check;
pub mod lint;
pub mod scene;
pub mod server;
pub mod store;
pub mod sync;
pub mod vrtx;
