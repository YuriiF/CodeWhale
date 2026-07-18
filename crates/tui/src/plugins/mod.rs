#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

pub mod discovery;
pub mod manifest;
pub mod registry;
pub mod types;

#[cfg(test)]
mod tests;

pub use registry::PluginRegistry;

/// Discover one immutable registry snapshot for one effective workspace.
///
/// The snapshot is deliberately caller-owned. TUI Apps, headless Engines, and
/// concurrent Runtime API threads must never share mutable plugin authority
/// through process-global state.
#[must_use]
pub fn registry_for_workspace(workspace: &Path) -> Arc<PluginRegistry> {
    Arc::new(discovery::discover_all(workspace))
}
