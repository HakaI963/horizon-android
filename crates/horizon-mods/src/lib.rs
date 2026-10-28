//! Ember, Horizon's mod loader (`docs/ember.md`, `docs/mods.md`): WebAssembly
//! components written against
//! `crates/horizon-mod-sdk/wit/horizon.wit`, each in its own sandbox with
//! no file, network or clock access, a memory cap and a time budget per
//! call. A mod that traps or overruns is turned off; the game goes on.
//!
//! Nothing here runs unless a mod is installed and turned on: an empty
//! [`Mods`] makes no engine and every hook returns at once.
//!
//! One loader, every platform. On a desktop the component is compiled to native
//! code; on Android, where the OS refuses executable memory, the same compiler is
//! pointed at the Pulley target and the interpreter runs the result. The sandbox is
//! the same either way, so an `.hmod` built once runs everywhere.
pub mod content;
#[cfg(feature = "index")]
pub mod index;
pub mod manifest;
pub mod modpack;
#[cfg(feature = "runtime")]
mod runtime;

pub use content::Content;
pub use manifest::{Capability, Manifest, Package, Side, find};
#[cfg(feature = "runtime")]
pub use runtime::*;
