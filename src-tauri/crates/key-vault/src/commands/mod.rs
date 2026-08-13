//! Tauri commands for credential validation
//!
//! Exposes validation functions to the frontend via Tauri's invoke system.

mod batch;
mod cli_version;
mod crud;
mod install;
mod prompt_polish;
pub mod registry;
mod validate;

pub use batch::*;
pub use cli_version::*;
pub use crud::*;
pub use install::*;
pub use prompt_polish::*;
pub use registry::*;
pub use validate::*;

#[cfg(test)]
mod tests;
