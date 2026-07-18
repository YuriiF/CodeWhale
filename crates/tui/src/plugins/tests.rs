use std::fs;
use std::path::{Path, PathBuf};

use super::discovery::{DiscoveryConfig, discover_with_config};
use super::types::PluginTrustStatus;

fn config(root: &Path) -> DiscoveryConfig {
    DiscoveryConfig {
        workspace: root.join("project"),
        user_plugins_dir: root.join("user"),
        workspace_plugins_dir: root.join("workspace"),
        builtin_plugin_dirs: Vec::new(),
        state_path: root.join("state/plugin-state.json"),
    }
}

fn write_plugin(config: &DiscoveryConfig, extra: &str) -> PathBuf {
    write_named_plugin(config, "demo", extra)
}

fn write_named_plugin(config: &DiscoveryConfig, name: &str, extra: &str) -> PathBuf {
    let plugin = config.user_plugins_dir.join(name);
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("plugin.toml"),
        format!("schema_version = 1\n[plugin]\nname = {name:?}\nversion = \"1.0.0\"\n{extra}"),
    )
    .unwrap();
    plugin
}

#[test]
fn trust_and_enablement_are_separate_atomic_state_transitions() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    write_plugin(&config, "");

    let mut registry = discover_with_config(&config);
    assert!(registry.enable("demo").is_err());
    assert!(!config.state_path.exists());

    registry.trust("demo").unwrap();
    assert!(registry.get("demo").unwrap().trusted());
    assert!(!registry.get("demo").unwrap().enabled);
    registry.enable("demo").unwrap();
    assert!(registry.is_active("demo"));
    registry.revoke_trust("demo").unwrap();
    assert!(registry.get("demo").unwrap().enabled);
    registry.trust("demo").unwrap();
    assert!(registry.get("demo").unwrap().trusted());
    assert!(
        !registry.get("demo").unwrap().enabled,
        "trust must never reuse an old enablement bit"
    );
    assert!(!registry.is_active("demo"));
    registry.enable("demo").unwrap();
    assert!(registry.is_active("demo"));

    let raw = fs::read_to_string(&config.state_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["schema_version"], 1);
    let receipt = parsed["plugins"]
        .as_object()
        .and_then(|plugins| plugins.values().next())
        .and_then(|plugin| plugin.get("trust"))
        .expect("trust receipt");
    assert!(receipt["content_hash"].as_str().is_some());
    assert!(receipt["capability_hash"].as_str().is_some());
    assert_eq!(receipt["reviewed_capabilities"]["skills"], 0);
    assert!(receipt["reviewed_at"].as_str().is_some());
    let history = parsed["plugins"]
        .as_object()
        .and_then(|plugins| plugins.values().next())
        .and_then(|plugin| plugin["review_history"].as_array())
        .expect("review history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[1]["content_hash"], receipt["content_hash"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&config.state_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let entries = fs::read_dir(config.state_path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(entries.iter().any(|name| name == "plugin-state.json"));
    assert!(entries.iter().any(|name| name == "plugin-state.json.lock"));
    assert!(entries.iter().any(|name| name == ".runtime"));
    assert!(
        entries.iter().all(|name| !name.contains(".tmp")),
        "atomic persistence must not strand temp files: {entries:?}"
    );
}

#[test]
fn content_change_invalidates_trust_without_changing_capabilities() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    let plugin = write_plugin(&config, "\n[skills]\npath = \"skills\"\n");
    fs::create_dir_all(plugin.join("skills/demo")).unwrap();
    fs::write(
        plugin.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: first\n---\nbody\n",
    )
    .unwrap();

    let mut first = discover_with_config(&config);
    first.trust("demo").unwrap();
    first.enable("demo").unwrap();
    assert!(first.is_active("demo"));

    fs::write(
        plugin.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: changed\n---\nbody\n",
    )
    .unwrap();
    let second = discover_with_config(&config);
    let plugin = second.get("demo").unwrap();
    assert!(plugin.enabled, "enablement is independent from trust");
    assert_eq!(plugin.trust_status, PluginTrustStatus::ContentChanged);
    assert!(!plugin.active());
}

#[test]
fn capability_escalation_invalidates_trust_and_stays_inactive() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    let plugin = write_plugin(&config, "");

    let mut first = discover_with_config(&config);
    first.trust("demo").unwrap();
    first.enable("demo").unwrap();

    fs::create_dir_all(plugin.join("hooks")).unwrap();
    fs::write(
        plugin.join("plugin.toml"),
        "schema_version = 1\n[plugin]\nname = \"demo\"\nversion = \"1.0.0\"\n[hooks]\npath = \"hooks\"\n",
    )
    .unwrap();
    let second = discover_with_config(&config);
    let plugin = second.get("demo").unwrap();
    assert_eq!(plugin.trust_status, PluginTrustStatus::CapabilitiesChanged);
    assert!(plugin.enabled);
    assert!(!plugin.active());
}

#[test]
fn malformed_state_is_fail_closed_and_never_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    write_plugin(&config, "");
    fs::create_dir_all(config.state_path.parent().unwrap()).unwrap();
    fs::write(&config.state_path, "{ malformed").unwrap();

    let mut registry = discover_with_config(&config);
    assert!(registry.state_error().is_some());
    assert!(!registry.get("demo").unwrap().enabled);
    assert!(!registry.get("demo").unwrap().trusted());
    assert!(registry.trust("demo").is_err());
    assert_eq!(
        fs::read_to_string(&config.state_path).unwrap(),
        "{ malformed"
    );
}

#[test]
fn atomic_write_failure_does_not_mutate_live_enablement() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    write_plugin(&config, "");

    let mut registry = discover_with_config(&config);
    registry.trust("demo").unwrap();
    fs::remove_file(&config.state_path).unwrap();
    fs::create_dir(&config.state_path).unwrap();

    assert!(registry.enable("demo").is_err());
    let plugin = registry.get("demo").unwrap();
    assert!(plugin.trusted());
    assert!(!plugin.enabled);
    assert!(!plugin.active());
}

#[test]
fn revoking_trust_does_not_rewrite_enablement() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    write_plugin(&config, "");

    let mut registry = discover_with_config(&config);
    registry.trust("demo").unwrap();
    registry.enable("demo").unwrap();
    registry.revoke_trust("demo").unwrap();

    let plugin = registry.get("demo").unwrap();
    assert!(plugin.enabled);
    assert!(!plugin.trusted());
    assert!(!plugin.active());
}

#[test]
fn unsupported_components_can_be_reviewed_but_not_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    let plugin = write_plugin(&config, "\n[commands]\npath = \"commands\"\n");
    fs::create_dir_all(plugin.join("commands")).unwrap();

    let mut registry = discover_with_config(&config);
    registry.trust("demo").unwrap();
    let error = registry.enable("demo").unwrap_err();
    assert!(error.contains("inactive capabilities"));
    assert!(!registry.is_active("demo"));
}

#[test]
fn stale_concurrent_registries_do_not_lose_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    write_named_plugin(&config, "alpha", "");
    write_named_plugin(&config, "beta", "");

    let left = discover_with_config(&config);
    let right = discover_with_config(&config);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let left_barrier = std::sync::Arc::clone(&barrier);
    let left = std::thread::spawn(move || {
        let mut registry = left;
        left_barrier.wait();
        registry.trust("alpha").unwrap();
        registry.enable("alpha").unwrap();
    });
    let right = std::thread::spawn(move || {
        let mut registry = right;
        barrier.wait();
        registry.trust("beta").unwrap();
        registry.enable("beta").unwrap();
    });
    left.join().unwrap();
    right.join().unwrap();

    let fresh = discover_with_config(&config);
    assert!(fresh.is_active("alpha"));
    assert!(fresh.is_active("beta"));
}

#[test]
fn stale_enable_cannot_resurrect_revoked_trust() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    write_plugin(&config, "");
    let mut initial = discover_with_config(&config);
    initial.trust("demo").unwrap();
    initial.enable("demo").unwrap();

    let mut stale = discover_with_config(&config);
    let authority = stale.authority_for("demo").unwrap();
    let mut revoker = discover_with_config(&config);
    revoker.revoke_trust("demo").unwrap();
    assert!(super::registry::verify_plugin_state_authority(&authority).is_err());

    stale.enable("demo").unwrap();
    let fresh = discover_with_config(&config);
    assert!(fresh.get("demo").unwrap().enabled);
    assert!(!fresh.get("demo").unwrap().trusted());
    assert!(!fresh.is_active("demo"));
}

#[cfg(unix)]
#[test]
fn staging_is_owner_only_and_uses_the_reviewed_executable_shape() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    let plugin = write_plugin(&config, "");
    let executable = plugin.join("server.sh");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

    let mut registry = discover_with_config(&config);
    registry.trust("demo").unwrap();
    registry.enable("demo").unwrap();
    let staged = registry.get("demo").unwrap().staged_root.as_ref().unwrap();
    assert_ne!(staged, &plugin);
    assert_eq!(
        fs::metadata(staged).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(staged.join("plugin.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
    assert_eq!(
        fs::metadata(staged.join("server.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
}

#[cfg(unix)]
#[test]
fn staging_rejects_root_swaps_symlinked_runtime_parents_and_hardlinks() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let config = config(tmp.path());
    let plugin = write_plugin(&config, "");
    let mut swapped = discover_with_config(&config);
    let original = plugin.with_file_name("demo-original");
    fs::rename(&plugin, &original).unwrap();
    let outside = tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(
        outside.join("plugin.toml"),
        "schema_version = 1\n[plugin]\nname = \"demo\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    symlink(&outside, &plugin).unwrap();
    assert!(swapped.trust("demo").is_err());

    fs::remove_file(&plugin).unwrap();
    fs::rename(&original, &plugin).unwrap();
    let mut parent_swap = discover_with_config(&config);
    fs::create_dir_all(config.state_path.parent().unwrap()).unwrap();
    let runtime_root = config.state_path.parent().unwrap().join(".runtime");
    if runtime_root.exists() {
        fs::remove_dir_all(&runtime_root).unwrap();
    }
    let runtime_outside = tmp.path().join("runtime-outside");
    fs::create_dir(&runtime_outside).unwrap();
    symlink(&runtime_outside, &runtime_root).unwrap();
    assert!(parent_swap.trust("demo").is_err());

    fs::remove_file(&runtime_root).unwrap();
    let external_file = tmp.path().join("external.txt");
    fs::write(&external_file, "reviewed-looking content").unwrap();
    fs::hard_link(&external_file, plugin.join("hardlinked.txt")).unwrap();
    let mut hardlinked = discover_with_config(&config);
    assert!(hardlinked.trust("demo").is_err());
}

#[test]
fn workspace_scoped_registries_do_not_cross_load_skills() {
    let tmp = tempfile::tempdir().unwrap();
    let left_config = config(&tmp.path().join("left"));
    let right_config = config(&tmp.path().join("right"));
    for (config, body) in [(&left_config, "left body"), (&right_config, "right body")] {
        let plugin = write_plugin(config, "\n[skills]\npath = \"skills\"\n");
        fs::create_dir_all(plugin.join("skills/only")).unwrap();
        fs::write(
            plugin.join("skills/only/SKILL.md"),
            format!("---\nname: only\ndescription: scoped\n---\n{body}\n"),
        )
        .unwrap();
    }
    let mut left = discover_with_config(&left_config);
    left.trust("demo").unwrap();
    left.enable("demo").unwrap();
    let mut right = discover_with_config(&right_config);
    right.trust("demo").unwrap();
    right.enable("demo").unwrap();

    let left_skills =
        crate::skills::discover_from_directories_with_plugins(Vec::<PathBuf>::new(), Some(&left));
    let right_skills =
        crate::skills::discover_from_directories_with_plugins(Vec::<PathBuf>::new(), Some(&right));
    assert_eq!(left_skills.get("demo:only").unwrap().body, "left body");
    assert_eq!(right_skills.get("demo:only").unwrap().body, "right body");
    assert_ne!(left.workspace(), right.workspace());
}
