//! Subcommand implementations.
//!
//! Each module owns its clap types and three things `main.rs` and `version` rely on:
//! - a `wants_json()` method saying whether the invocation asked for `--json` (machine mode);
//! - `pub const CAPABILITIES`, the contract capabilities it implements (DESIGN §5.3);
//! - human text printed through `humanln!`/`human!` and JSON through `output::emit_json`.
pub mod agent;
pub mod config;
pub mod detect;
pub mod devcontainer;
pub mod doctor;
pub mod feature;
pub mod init;
pub mod tunnel;
pub mod version;
