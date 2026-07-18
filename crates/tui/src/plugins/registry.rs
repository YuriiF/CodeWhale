use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::manifest::PluginInventory;
use super::types::{
    LoadedPlugin, PluginAuthority, PluginDiagnostic, PluginDiagnosticLevel, PluginId,
    PluginTrustStatus,
};

const STATE_SCHEMA_VERSION: u32 = 1;
const MAX_REVIEW_HISTORY: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginStateFile {
    schema_version: u32,
    #[serde(default)]
    plugins: BTreeMap<PluginId, PersistedPluginState>,
}

impl Default for PluginStateFile {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            plugins: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedPluginState {
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    trust: Option<TrustReceipt>,
    #[serde(default)]
    review_history: Vec<TrustReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustReceipt {
    content_hash: String,
    capability_hash: String,
    reviewed_capabilities: PluginInventory,
    reviewed_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    plugins: BTreeMap<PluginId, LoadedPlugin>,
    names: BTreeMap<String, PluginId>,
    diagnostics: Vec<PluginDiagnostic>,
    state: PluginStateFile,
    state_path: Option<PathBuf>,
    state_error: Option<String>,
    workspace: PathBuf,
}

impl PluginRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_discovery(
        plugins: Vec<LoadedPlugin>,
        mut diagnostics: Vec<PluginDiagnostic>,
        state_path: PathBuf,
        workspace: PathBuf,
    ) -> Self {
        let (state, state_error) = match load_state(&state_path) {
            Ok(state) => (state, None),
            Err(error) => {
                diagnostics.push(PluginDiagnostic::error(
                    "state-invalid",
                    format!("Plugin state is fail-closed and will not be overwritten: {error}"),
                    Some(state_path.clone()),
                ));
                (PluginStateFile::default(), Some(error))
            }
        };
        let mut registry = Self {
            plugins: BTreeMap::new(),
            names: BTreeMap::new(),
            diagnostics,
            state,
            state_path: Some(state_path),
            state_error,
            workspace,
        };
        for plugin in plugins {
            registry.register_loaded(plugin);
        }
        registry.apply_state();
        registry
    }

    fn register_loaded(&mut self, plugin: LoadedPlugin) {
        self.names
            .insert(plugin.name().to_string(), plugin.id.clone());
        self.plugins.insert(plugin.id.clone(), plugin);
    }

    fn apply_state(&mut self) {
        let state_path = self.state_path.clone();
        for (id, plugin) in &mut self.plugins {
            let persisted = self.state.plugins.get(id);
            plugin.state_generation = persisted.map_or(0, |state| state.generation);
            plugin.enabled = persisted.is_some_and(|state| state.enabled);
            plugin.trust_status = match persisted.and_then(|state| state.trust.as_ref()) {
                Some(receipt) if receipt.capability_hash != plugin.capability_hash => {
                    PluginTrustStatus::CapabilitiesChanged
                }
                Some(receipt) if receipt.content_hash != plugin.content_hash => {
                    PluginTrustStatus::ContentChanged
                }
                Some(_) => PluginTrustStatus::Trusted,
                None => PluginTrustStatus::NeverReviewed,
            };
            if self.state_error.is_some() {
                plugin.enabled = false;
                plugin.trust_status = PluginTrustStatus::NeverReviewed;
            }
            plugin.staged_root = state_path.as_deref().and_then(|state_path| {
                let staged_root = runtime_stage_path(state_path, id, &plugin.content_hash);
                staged_bundle_matches(&staged_root, &plugin.content_hash, &plugin.capability_hash)
                    .then_some(staged_root)
            });
        }
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    #[must_use]
    pub fn authority_for(&self, selector: &str) -> Option<PluginAuthority> {
        self.get(selector)
            .and_then(|plugin| plugin.authority(self.state_path.clone()?, self.workspace.clone()))
    }

    #[must_use]
    pub fn list(&self) -> Vec<&LoadedPlugin> {
        let mut plugins = self.plugins.values().collect::<Vec<_>>();
        plugins.sort_by(|left, right| {
            left.scope
                .cmp(&right.scope)
                .then_with(|| left.name().cmp(right.name()))
                .then_with(|| left.id.cmp(&right.id))
        });
        plugins
    }

    #[must_use]
    pub fn get(&self, selector: &str) -> Option<&LoadedPlugin> {
        let id = self.resolve_id(selector)?;
        self.plugins.get(id)
    }

    #[must_use]
    pub fn active_plugins(&self) -> Vec<&LoadedPlugin> {
        self.list()
            .into_iter()
            .filter(|plugin| plugin.active())
            .collect()
    }

    /// Compatibility name retained for the MCP adapter. Unlike the old
    /// registry this returns only trusted, active bundles.
    #[must_use]
    pub fn list_enabled(&self) -> Vec<&LoadedPlugin> {
        self.active_plugins()
    }

    #[must_use]
    pub fn enabled_plugins(&self) -> Vec<&LoadedPlugin> {
        self.list()
            .into_iter()
            .filter(|plugin| plugin.enabled)
            .collect()
    }

    #[must_use]
    pub fn is_enabled(&self, selector: &str) -> bool {
        self.get(selector).is_some_and(|plugin| plugin.enabled)
    }

    #[must_use]
    pub fn is_active(&self, selector: &str) -> bool {
        self.get(selector).is_some_and(LoadedPlugin::active)
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[PluginDiagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn validation_is_clean(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == PluginDiagnosticLevel::Error)
            && self.plugins.values().all(|plugin| {
                !plugin
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.level == PluginDiagnosticLevel::Error)
            })
    }

    #[must_use]
    pub fn state_error(&self) -> Option<&str> {
        self.state_error.as_deref()
    }

    #[must_use]
    pub fn state_path(&self) -> Option<&Path> {
        self.state_path.as_deref()
    }

    pub fn trust(&mut self, selector: &str) -> Result<(), String> {
        let plugin = self
            .get(selector)
            .ok_or_else(|| format!("Plugin bundle `{selector}` was not found"))?;
        let plugin = plugin.clone();
        let id = plugin.id.clone();
        let state_path = self
            .state_path
            .as_deref()
            .ok_or_else(|| "Plugin registry has no persistence store".to_string())?;
        stage_bundle(state_path, &plugin)?;
        let receipt = TrustReceipt {
            content_hash: plugin.content_hash.clone(),
            capability_hash: plugin.capability_hash.clone(),
            reviewed_capabilities: plugin.inventory.clone(),
            reviewed_at: chrono::Utc::now().to_rfc3339(),
        };
        self.commit_state_change(|state| {
            let entry = state.plugins.entry(id).or_default();
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or_else(|| "Plugin authority generation is exhausted".to_string())?;
            // Trust records review and staging only. Even if an older state
            // kept the enablement bit across revocation or content drift,
            // re-review must never reactivate the bundle implicitly.
            entry.enabled = false;
            entry.trust = Some(receipt.clone());
            entry.review_history.push(receipt);
            if entry.review_history.len() > MAX_REVIEW_HISTORY {
                let remove = entry.review_history.len() - MAX_REVIEW_HISTORY;
                entry.review_history.drain(..remove);
            }
            Ok(())
        })
    }

    pub fn revoke_trust(&mut self, selector: &str) -> Result<(), String> {
        let id = self
            .resolve_id(selector)
            .cloned()
            .ok_or_else(|| format!("Plugin bundle `{selector}` was not found"))?;
        self.commit_state_change(|state| {
            let entry = state.plugins.entry(id).or_default();
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or_else(|| "Plugin authority generation is exhausted".to_string())?;
            entry.trust = None;
            Ok(())
        })
    }

    pub fn enable(&mut self, selector: &str) -> Result<(), String> {
        let plugin = self
            .get(selector)
            .ok_or_else(|| format!("Plugin bundle `{selector}` was not found"))?;
        if !plugin.trusted() {
            return Err(format!(
                "Plugin bundle `{}` requires capability review before enablement (trust: {})",
                plugin.name(),
                plugin.trust_status.as_str()
            ));
        }
        if plugin.staged_root.is_none() {
            return Err(format!(
                "Plugin bundle `{}` has no verified Codewhale runtime snapshot; review and trust it again before enablement",
                plugin.name()
            ));
        }
        if !plugin.applicable {
            return Err(format!(
                "Plugin bundle `{}` does not apply to this host",
                plugin.name()
            ));
        }
        let unsupported = plugin.inventory.unsupported_labels();
        if !unsupported.is_empty() {
            return Err(format!(
                "Plugin bundle `{}` declares v0.9.1-inactive capabilities: {}",
                plugin.name(),
                unsupported.join(", ")
            ));
        }
        let id = plugin.id.clone();
        self.commit_state_change(|state| {
            let entry = state.plugins.entry(id).or_default();
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or_else(|| "Plugin authority generation is exhausted".to_string())?;
            entry.enabled = true;
            Ok(())
        })
    }

    pub fn disable(&mut self, selector: &str) -> Result<(), String> {
        let id = self
            .resolve_id(selector)
            .cloned()
            .ok_or_else(|| format!("Plugin bundle `{selector}` was not found"))?;
        self.commit_state_change(|state| {
            let entry = state.plugins.entry(id).or_default();
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or_else(|| "Plugin authority generation is exhausted".to_string())?;
            entry.enabled = false;
            Ok(())
        })
    }

    fn commit_state_change(
        &mut self,
        mutate: impl FnOnce(&mut PluginStateFile) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(error) = &self.state_error {
            return Err(format!(
                "Plugin state is fail-closed; repair or move the malformed state file before mutating it: {error}"
            ));
        }
        let Some(path) = self.state_path.as_deref() else {
            return Err("Plugin registry has no persistence store".to_string());
        };
        let lock_path = state_lock_path(path);
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create plugin state directory: {e}"))?;
        }
        let lock_file = open_state_lock(&lock_path, true)?;
        let mut lock = fd_lock::RwLock::new(lock_file);
        let _guard = lock
            .write()
            .map_err(|e| format!("failed to lock plugin state for update: {e}"))?;
        let mut next = load_state_unlocked(path)?;
        mutate(&mut next)?;
        save_state(path, &next)?;
        self.state = next;
        self.apply_state();
        Ok(())
    }

    fn resolve_id(&self, selector: &str) -> Option<&PluginId> {
        self.plugins
            .keys()
            .find(|id| id.as_str() == selector)
            .or_else(|| self.names.get(selector))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

fn load_state(path: &Path) -> Result<PluginStateFile, String> {
    let lock_path = state_lock_path(path);
    if lock_path.exists() {
        let lock_file = open_state_lock(&lock_path, false)?;
        let lock = fd_lock::RwLock::new(lock_file);
        let _guard = lock
            .read()
            .map_err(|e| format!("failed to read-lock plugin state: {e}"))?;
        return load_state_unlocked(path);
    }
    load_state_unlocked(path)
}

fn load_state_unlocked(path: &Path) -> Result<PluginStateFile, String> {
    if !path.exists() {
        return Ok(PluginStateFile::default());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let state: PluginStateFile = serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported plugin state schema {}; expected {STATE_SCHEMA_VERSION}",
            state.schema_version
        ));
    }
    Ok(state)
}

fn save_state(path: &Path, state: &PluginStateFile) -> Result<(), String> {
    codewhale_config::persistence::atomic_write_json(path, state)
        .map_err(|e| format!("failed to atomically persist {}: {e}", path.display()))
}

fn state_lock_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "state.json".into());
    name.push(".lock");
    path.with_file_name(name)
}

fn open_state_lock(path: &Path, create: bool) -> Result<fs::File, String> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| format!("failed to open plugin state lock: {e}"))
}

fn runtime_stage_path(state_path: &Path, id: &PluginId, content_hash: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(b"codewhale-plugin-stage-v1\0");
    hasher.update(id.as_str().as_bytes());
    let key = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let state_parent = state_path.parent().unwrap_or_else(|| Path::new("."));
    let state_parent = state_parent
        .canonicalize()
        .unwrap_or_else(|_| state_parent.to_path_buf());
    state_parent
        .join(".runtime")
        .join("v1")
        .join(key)
        .join(content_hash)
}

fn staged_bundle_matches(root: &Path, content_hash: &str, capability_hash: &str) -> bool {
    super::manifest::PluginManifest::validate_from_path(&root.join("plugin.toml")).is_ok_and(
        |validated| {
            validated.content_hash == content_hash
                && validated.capability_hash == capability_hash
                && root
                    .canonicalize()
                    .is_ok_and(|root| validated.canonical_root == root)
        },
    )
}

fn stage_bundle(state_path: &Path, plugin: &LoadedPlugin) -> Result<PathBuf, String> {
    // Resolve the state directory before deriving the content-addressed path.
    // On macOS an existing ancestor such as `/var` canonicalizes to
    // `/private/var`; when the final `state/` directory does not exist yet,
    // deriving the destination first would preserve the non-canonical prefix
    // and the subsequent containment proof would correctly reject it as an
    // escape. Trust is already the mutating boundary, so creating this private
    // parent here is both safe and necessary for a stable path identity.
    let state_parent = state_path
        .parent()
        .ok_or_else(|| "plugin state path has no parent directory".to_string())?;
    fs::create_dir_all(state_parent)
        .map_err(|e| format!("failed to create plugin state directory: {e}"))?;
    let destination = runtime_stage_path(state_path, &plugin.id, &plugin.content_hash);
    if destination.exists() {
        return staged_bundle_matches(
            &destination,
            &plugin.content_hash,
            &plugin.capability_hash,
        )
        .then(|| destination.canonicalize().unwrap_or(destination))
        .ok_or_else(|| {
            "Existing Codewhale plugin runtime snapshot failed content validation; remove the exact .runtime entry and review again"
                .to_string()
        });
    }

    let parent = destination
        .parent()
        .ok_or_else(|| "plugin runtime snapshot has no parent".to_string())?;
    ensure_private_runtime_parent(state_path, parent)?;
    let temporary = parent.join(format!(".staging-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&temporary)
        .map_err(|e| format!("failed to create temporary plugin runtime snapshot: {e}"))?;
    set_owner_only_directory(&temporary)?;

    let staged = (|| {
        copy_bundle_tree(&plugin.canonical_root, &temporary)?;
        if !staged_bundle_matches(&temporary, &plugin.content_hash, &plugin.capability_hash) {
            return Err(
                "Plugin bundle changed while Codewhale was staging it; no runtime authority was granted"
                    .to_string(),
            );
        }
        harden_staged_tree(&temporary)?;
        fs::rename(&temporary, &destination).map_err(|e| {
            format!("failed to activate content-addressed plugin runtime snapshot: {e}")
        })?;
        destination
            .canonicalize()
            .map_err(|e| format!("failed to finalize plugin runtime snapshot path: {e}"))
    })();
    if staged.is_err() && temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    if staged.is_err()
        && destination.exists()
        && staged_bundle_matches(&destination, &plugin.content_hash, &plugin.capability_hash)
    {
        return Ok(destination);
    }
    staged
}

fn ensure_private_runtime_parent(state_path: &Path, parent: &Path) -> Result<(), String> {
    let configured_base = state_path
        .parent()
        .ok_or_else(|| "plugin state path has no parent directory".to_string())?;
    fs::create_dir_all(configured_base)
        .map_err(|e| format!("failed to create plugin state directory: {e}"))?;
    // `runtime_stage_path` canonicalizes the same parent. Match that identity
    // here as well (notably `/var` -> `/private/var` on macOS) before proving
    // that every runtime component stays beneath the state directory.
    let base = configured_base
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize plugin state directory: {e}"))?;
    let relative = parent
        .strip_prefix(&base)
        .map_err(|_| "plugin runtime snapshot escaped the state directory".to_string())?;
    let mut cursor = base;
    for component in relative.components() {
        use std::path::Component;
        let Component::Normal(component) = component else {
            return Err("plugin runtime snapshot contains an invalid path component".to_string());
        };
        cursor.push(component);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(
                    "plugin runtime snapshot directory may not traverse symbolic links".to_string(),
                );
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("plugin runtime snapshot parent is not a directory".to_string());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&cursor) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&cursor).map_err(|e| {
                            format!(
                                "failed to inspect concurrently created plugin runtime snapshot directory: {e}"
                            )
                        })?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(
                                "concurrently created plugin runtime snapshot parent is not a safe directory"
                                    .to_string(),
                            );
                        }
                    }
                    Err(error) => {
                        return Err(format!(
                            "failed to create plugin runtime snapshot directory: {error}"
                        ));
                    }
                }
            }
            Err(error) => {
                return Err(format!(
                    "failed to inspect plugin runtime snapshot directory: {error}"
                ));
            }
        }
        set_owner_only_directory(&cursor)?;
    }
    Ok(())
}

#[derive(Default)]
struct StageBudget {
    files: usize,
    bytes: u64,
}

fn copy_bundle_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let mut budget = StageBudget::default();
    copy_bundle_tree_bounded(source, destination, &mut budget)
}

#[cfg(not(unix))]
fn copy_bundle_tree_bounded(
    source: &Path,
    destination: &Path,
    budget: &mut StageBudget,
) -> Result<(), String> {
    use std::io::Read as _;
    let metadata = fs::symlink_metadata(source)
        .map_err(|e| format!("failed to inspect plugin content during staging: {e}"))?;
    if metadata.file_type().is_symlink() {
        return Err("Plugin content changed into a symbolic link during staging".to_string());
    }
    if !metadata.is_dir() {
        return Err("Plugin runtime source is not a directory".to_string());
    }
    let mut entries = fs::read_dir(source)
        .map_err(|e| format!("failed to read plugin content during staging: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to enumerate plugin content during staging: {e}"))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|e| format!("failed to inspect plugin entry during staging: {e}"))?;
        if metadata.file_type().is_symlink() {
            return Err("Plugin content may not contain symbolic links".to_string());
        }
        if metadata.is_dir() {
            fs::create_dir(&destination_path)
                .map_err(|e| format!("failed to create staged plugin directory: {e}"))?;
            set_owner_only_directory(&destination_path)?;
            copy_bundle_tree_bounded(&source_path, &destination_path, budget)?;
        } else if metadata.is_file() {
            budget.files = budget.files.saturating_add(1);
            if budget.files > 4_096 {
                return Err("Plugin content exceeded the staging file limit".to_string());
            }
            let mut source_file = super::manifest::open_bundle_file(&source_path)
                .map_err(|e| format!("failed to open plugin file without following links: {e}"))?;
            let mut destination_file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&destination_path)
                .map_err(|e| format!("failed to create staged plugin file: {e}"))?;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = source_file
                    .read(&mut buffer)
                    .map_err(|e| format!("failed to read plugin file during staging: {e}"))?;
                if read == 0 {
                    break;
                }
                budget.bytes = budget.bytes.saturating_add(read as u64);
                if budget.bytes > 64 * 1024 * 1024 {
                    return Err("Plugin content exceeded the staging byte limit".to_string());
                }
                destination_file
                    .write_all(&buffer[..read])
                    .map_err(|e| format!("failed to write staged plugin file: {e}"))?;
            }
            destination_file
                .sync_all()
                .map_err(|e| format!("failed to sync staged plugin file: {e}"))?;
            preserve_owner_only_file_mode(&destination_path, &metadata)?;
        } else {
            return Err(
                "Plugin content must contain only regular files and directories".to_string(),
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_bundle_tree_bounded(
    source: &Path,
    destination: &Path,
    budget: &mut StageBudget,
) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| "plugin runtime source path contains an invalid byte".to_string())?;
    // SAFETY: `source` is a NUL-terminated path and successful descriptors
    // are immediately owned by `OwnedFd`.
    let fd = unsafe {
        libc::open(
            source.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(format!(
            "failed to open plugin root without following links: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `fd` is a unique successful result from `open` above.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    copy_bundle_directory_fd(fd.as_raw_fd(), destination, budget)
}

#[cfg(unix)]
fn copy_bundle_directory_fd(
    source_fd: std::os::fd::RawFd,
    destination: &Path,
    budget: &mut StageBudget,
) -> Result<(), String> {
    use std::ffi::{CStr, CString, OsString};
    use std::io::Read as _;
    use std::mem::MaybeUninit;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStringExt;

    // `fdopendir` owns its descriptor, so duplicate the directory fd retained
    // by this stack frame for subsequent `openat` calls.
    // SAFETY: `source_fd` is an open directory descriptor.
    let iter_fd = unsafe { libc::dup(source_fd) };
    if iter_fd < 0 {
        return Err(format!(
            "failed to duplicate plugin directory descriptor: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `iter_fd` is a fresh descriptor and ownership transfers to DIR.
    let directory = unsafe { libc::fdopendir(iter_fd) };
    if directory.is_null() {
        // SAFETY: fdopendir failed, so ownership did not transfer.
        unsafe { libc::close(iter_fd) };
        return Err(format!(
            "failed to enumerate plugin directory safely: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut names = Vec::new();
    loop {
        // SAFETY: `directory` remains valid until closed below.
        let entry = unsafe { libc::readdir(directory) };
        if entry.is_null() {
            break;
        }
        // SAFETY: POSIX dirent d_name is NUL-terminated for returned entries.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        names.push(OsString::from_vec(name.to_vec()));
    }
    // SAFETY: closes DIR and its duplicated descriptor exactly once.
    unsafe { libc::closedir(directory) };
    names.sort();

    for name in names {
        let name_c = CString::new(name.clone().into_vec())
            .map_err(|_| "plugin entry name contains an invalid byte".to_string())?;
        let mut stat = MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: source_fd and name are valid; stat points to writable memory.
        if unsafe {
            libc::fstatat(
                source_fd,
                name_c.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(format!(
                "failed to inspect plugin entry safely: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: fstatat initialized stat after returning success.
        let stat = unsafe { stat.assume_init() };
        let kind = stat.st_mode & libc::S_IFMT;
        let destination_path = destination.join(&name);
        if kind == libc::S_IFDIR {
            // SAFETY: openat is anchored to the already-open parent and
            // O_NOFOLLOW prevents a concurrent directory-to-symlink swap.
            let child_fd = unsafe {
                libc::openat(
                    source_fd,
                    name_c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if child_fd < 0 {
                return Err(format!(
                    "failed to open plugin directory safely: {}",
                    std::io::Error::last_os_error()
                ));
            }
            // SAFETY: unique descriptor returned by openat.
            let child_fd = unsafe { OwnedFd::from_raw_fd(child_fd) };
            fs::create_dir(&destination_path)
                .map_err(|e| format!("failed to create staged plugin directory: {e}"))?;
            set_owner_only_directory(&destination_path)?;
            copy_bundle_directory_fd(child_fd.as_raw_fd(), &destination_path, budget)?;
        } else if kind == libc::S_IFREG {
            if stat.st_nlink != 1 {
                return Err("Plugin content may not contain hard-linked files".to_string());
            }
            budget.files = budget.files.saturating_add(1);
            if budget.files > 4_096 {
                return Err("Plugin content exceeded the staging file limit".to_string());
            }
            // SAFETY: openat is anchored and O_NOFOLLOW prevents a file swap
            // to a symbolic link between metadata inspection and open.
            let file_fd = unsafe {
                libc::openat(
                    source_fd,
                    name_c.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if file_fd < 0 {
                return Err(format!(
                    "failed to open plugin file safely: {}",
                    std::io::Error::last_os_error()
                ));
            }
            // SAFETY: unique descriptor returned by openat.
            let mut source_file = unsafe { fs::File::from_raw_fd(file_fd) };
            let opened = source_file
                .metadata()
                .map_err(|e| format!("failed to inspect opened plugin file: {e}"))?;
            if !opened.is_file() {
                return Err("Plugin entry changed type during staging".to_string());
            }
            let mut destination_file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&destination_path)
                .map_err(|e| format!("failed to create staged plugin file: {e}"))?;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = source_file
                    .read(&mut buffer)
                    .map_err(|e| format!("failed to read plugin file during staging: {e}"))?;
                if read == 0 {
                    break;
                }
                budget.bytes = budget.bytes.saturating_add(read as u64);
                if budget.bytes > 64 * 1024 * 1024 {
                    return Err("Plugin content exceeded the staging byte limit".to_string());
                }
                destination_file
                    .write_all(&buffer[..read])
                    .map_err(|e| format!("failed to write staged plugin file: {e}"))?;
            }
            destination_file
                .sync_all()
                .map_err(|e| format!("failed to sync staged plugin file: {e}"))?;
            preserve_owner_only_file_mode(&destination_path, &opened)?;
        } else if kind == libc::S_IFLNK {
            return Err("Plugin content may not contain symbolic links".to_string());
        } else {
            return Err(
                "Plugin content must contain only regular files and directories".to_string(),
            );
        }
    }
    Ok(())
}

fn harden_staged_tree(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("failed to harden staged plugin content: {e}"))?;
    if metadata.is_dir() {
        let entries = fs::read_dir(path)
            .map_err(|e| format!("failed to read staged plugin content: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("failed to enumerate staged plugin content: {e}"))?;
        for entry in entries {
            harden_staged_tree(&entry.path())?;
        }
        set_owner_only_directory(path)?;
    } else if metadata.is_file() {
        preserve_owner_only_file_mode(path, &metadata)?;
    } else {
        return Err("Staged plugin content changed type before activation".to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| {
        format!(
            "failed to restrict plugin runtime directory permissions for {}: {e}",
            path.display()
        )
    })
}

#[cfg(windows)]
fn set_owner_only_directory(path: &Path) -> Result<(), String> {
    set_windows_owner_only_acl(path)
}

#[cfg(windows)]
fn set_windows_owner_only_acl(path: &Path) -> Result<(), String> {
    use std::mem::{MaybeUninit, size_of};
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WIN32_ERROR};
    use windows::Win32::Security::Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW};
    use windows::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION,
        GetLengthSid, GetTokenInformation, InitializeAcl, OBJECT_INHERIT_ACE,
        PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PCWSTR;

    let mut token = HANDLE::default();
    // SAFETY: output handle points to valid storage and the pseudo process
    // handle is valid for the current process.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|error| format!("failed to open the current Windows security token: {error}"))?;
    let result = (|| {
        let mut required = 0_u32;
        // The first call intentionally obtains the required byte count.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut required) };
        if required < size_of::<TOKEN_USER>() as u32 {
            return Err("Windows token did not expose a current-user SID".to_string());
        }
        let words = (required as usize).div_ceil(size_of::<usize>());
        let mut token_buffer = vec![MaybeUninit::<usize>::zeroed(); words];
        // SAFETY: aligned buffer is at least `required` bytes and remains alive
        // for every SID/ACL operation below.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(token_buffer.as_mut_ptr().cast()),
                required,
                &mut required,
            )
        }
        .map_err(|error| format!("failed to read the current Windows user SID: {error}"))?;
        // SAFETY: successful TokenUser query initialized a TOKEN_USER at the
        // beginning of the aligned buffer.
        let token_user = unsafe { &*token_buffer.as_ptr().cast::<TOKEN_USER>() };
        let sid = token_user.User.Sid;
        // SAFETY: SID comes from the successful token query above.
        let sid_len = unsafe { GetLengthSid(sid) } as usize;
        if sid_len == 0 {
            return Err("Windows current-user SID is invalid".to_string());
        }
        let acl_bytes =
            size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid_len;
        let acl_words = acl_bytes.div_ceil(size_of::<usize>());
        let mut acl_buffer = vec![MaybeUninit::<usize>::zeroed(); acl_words];
        let acl = acl_buffer.as_mut_ptr().cast::<ACL>();
        // SAFETY: aligned ACL buffer is large enough for one full-access ACE
        // containing the current user SID.
        unsafe { InitializeAcl(acl, acl_bytes as u32, ACL_REVISION) }
            .map_err(|error| format!("failed to initialize a private Windows ACL: {error}"))?;
        unsafe {
            windows::Win32::Security::AddAccessAllowedAceEx(
                acl,
                ACL_REVISION,
                CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE,
                0x001f_01ff,
                sid,
            )
        }
        .map_err(|error| format!("failed to grant the current Windows user access: {error}"))?;

        let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
        wide.push(0);
        // SAFETY: path is NUL terminated; ACL and SID buffers remain alive for
        // the call. A protected DACL prevents inherited broad access.
        let status = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(acl),
                None,
            )
        };
        if status != WIN32_ERROR(0) {
            return Err(format!(
                "failed to restrict Windows plugin runtime ACL: error {}",
                status.0
            ));
        }
        Ok(())
    })();
    // SAFETY: token is the unique real handle returned by OpenProcessToken.
    let _ = unsafe { CloseHandle(token) };
    result
}

#[cfg(all(not(unix), not(windows)))]
fn set_owner_only_directory(_path: &Path) -> Result<(), String> {
    Err("Plugin runtime staging is unavailable on this platform because owner-only filesystem permissions cannot be enforced".to_string())
}

#[cfg(unix)]
fn preserve_owner_only_file_mode(path: &Path, source: &fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let executable = source.permissions().mode() & 0o111 != 0;
    let mode = if executable { 0o500 } else { 0o400 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("failed to restrict staged plugin file permissions: {e}"))
}

#[cfg(not(unix))]
fn preserve_owner_only_file_mode(path: &Path, _source: &fs::Metadata) -> Result<(), String> {
    let mut permissions = fs::metadata(path)
        .map_err(|e| format!("failed to inspect staged plugin file permissions: {e}"))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)
        .map_err(|e| format!("failed to restrict staged plugin file permissions: {e}"))
}

/// Recheck a persisted plugin receipt, the mutable reviewed source, and the
/// Codewhale-owned immutable runtime copy. This function performs no writes.
pub fn verify_plugin_authority(authority: &PluginAuthority) -> Result<(), String> {
    verify_plugin_state_authority(authority)?;
    for (label, manifest_path) in [
        ("reviewed source", &authority.source_manifest),
        ("Codewhale runtime snapshot", &authority.staged_manifest),
    ] {
        let current =
            super::manifest::PluginManifest::validate_from_path(manifest_path).map_err(|_| {
                format!(
                    "Plugin bundle `{}` {label} could not be revalidated",
                    authority.plugin_name
                )
            })?;
        if current.content_hash != authority.content_hash
            || current.capability_hash != authority.capability_hash
        {
            return Err(format!(
                "Plugin bundle `{}` {label} changed after review",
                authority.plugin_name
            ));
        }
    }
    Ok(())
}

/// Cheap cross-process revocation probe used while an established MCP request
/// is in flight. Full source/stage hashing is intentionally done before each
/// dispatch; the watcher only needs to notice the locked state transition.
pub fn verify_plugin_state_authority(authority: &PluginAuthority) -> Result<(), String> {
    let lock_path = state_lock_path(&authority.state_path);
    let lock_file = open_state_lock(&lock_path, false).map_err(|_| {
        "Plugin authority state lock is missing; review and enable the bundle again".to_string()
    })?;
    let lock = fd_lock::RwLock::new(lock_file);
    let _guard = lock
        .read()
        .map_err(|_| "Plugin authority state could not be read safely".to_string())?;
    let state = load_state_unlocked(&authority.state_path).map_err(|_| {
        "Plugin authority state is invalid; the bundle is disabled fail-closed".to_string()
    })?;
    let active = state
        .plugins
        .get(&authority.plugin_id)
        .is_some_and(|entry| {
            entry.generation == authority.state_generation
                && entry.enabled
                && entry.trust.as_ref().is_some_and(|receipt| {
                    receipt.content_hash == authority.content_hash
                        && receipt.capability_hash == authority.capability_hash
                })
        });
    if !active {
        return Err(format!(
            "Plugin bundle `{}` is disabled, revoked, or no longer matches its review receipt",
            authority.plugin_name
        ));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_acl_tests {
    use super::set_windows_owner_only_acl;
    use std::ffi::c_void;
    use std::mem::{MaybeUninit, size_of};
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, CONTAINER_INHERIT_ACE,
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetFileSecurityW,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, OBJECT_INHERIT_ACE,
        PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
    };
    use windows::core::{BOOL, PCWSTR};

    #[test]
    fn owner_only_runtime_acl_is_protected_and_has_one_full_access_ace() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = directory.path().join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        set_windows_owner_only_acl(&runtime).unwrap();

        let mut wide = runtime.as_os_str().encode_wide().collect::<Vec<_>>();
        wide.push(0);
        let mut required = 0_u32;
        // SAFETY: this size-probe intentionally supplies no destination buffer.
        let _ = unsafe {
            GetFileSecurityW(
                PCWSTR(wide.as_ptr()),
                DACL_SECURITY_INFORMATION.0,
                None,
                0,
                &mut required,
            )
        };
        assert!(
            required > 0,
            "Windows did not report a security descriptor size"
        );
        let words = (required as usize).div_ceil(size_of::<usize>());
        let mut descriptor = vec![MaybeUninit::<usize>::zeroed(); words];
        let descriptor = PSECURITY_DESCRIPTOR(descriptor.as_mut_ptr().cast::<c_void>());
        // SAFETY: the aligned destination is at least `required` bytes and the
        // UTF-16 path remains NUL terminated for the call.
        assert!(
            unsafe {
                GetFileSecurityW(
                    PCWSTR(wide.as_ptr()),
                    DACL_SECURITY_INFORMATION.0,
                    Some(descriptor),
                    required,
                    &mut required,
                )
            }
            .as_bool()
        );

        let mut present = BOOL::default();
        let mut defaulted = BOOL::default();
        let mut acl = std::ptr::null_mut::<ACL>();
        // SAFETY: `descriptor` contains the successful GetFileSecurityW result.
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
            .unwrap();
        assert!(present.as_bool());
        assert!(!acl.is_null());

        let mut info = ACL_SIZE_INFORMATION::default();
        // SAFETY: `acl` is owned by the live descriptor buffer above.
        unsafe {
            GetAclInformation(
                acl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        }
        .unwrap();
        assert_eq!(info.AceCount, 1, "runtime DACL must name only the owner");

        let mut ace = std::ptr::null_mut::<c_void>();
        // SAFETY: the ACL contains exactly one ACE.
        unsafe { GetAce(acl, 0, &mut ace) }.unwrap();
        let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        assert_eq!(ace.Header.AceType, 0, "owner entry must be an allow ACE");
        assert_eq!(ace.Mask, 0x001f_01ff, "owner entry must grant full access");
        let inheritance = (CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE).0 as u8;
        assert_eq!(ace.Header.AceFlags & inheritance, inheritance);

        let mut control = 0_u16;
        let mut revision = 0_u32;
        // SAFETY: the descriptor buffer remains alive for this inspection.
        unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }.unwrap();
        assert_ne!(
            control & SE_DACL_PROTECTED.0,
            0,
            "runtime DACL must not inherit broader parent permissions"
        );
    }
}
