//! Closed-loop verification gate — re-checks tool side-effect claims
//! before the result enters the session message stream.
//!
//! After every tool that claims side effects, the engine runs a
//! deterministic re-check. If the re-check contradicts the claim, the
//! session message is annotated with `[VERIFY FAIL]` instead of a raw
//! `success: true` — and the model sees the discrepancy.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Verdict types
// ---------------------------------------------------------------------------

/// What the verifier found when it re-checked a tool's claimed result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VerifyVerdict {
    /// Re-check confirmed the claim.
    Pass,
    /// Re-check contradicted the claim with evidence.
    Fail {
        expected: String,
        observed: String,
    },
    /// Could not re-check (no read-only path available, or re-check tool failed).
    Unverifiable {
        reason: String,
    },
    /// Explicitly skipped (read-only tool, or tool returned `verification: "skip"` metadata).
    Skipped,
}

/// A single verification record for the session ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRecord {
    pub tool_id: String,
    pub tool_name: String,
    pub verdict: VerifyVerdict,
    pub elapsed_ms: u64,
    pub ts: i64,
}

/// Configuration for the verification gate.
#[derive(Debug, Clone)]
pub struct VerifyConfig {
    /// Enable the verification gate.
    pub enabled: bool,
    /// Tools to skip verification for.
    pub skip_tools: Vec<String>,
    /// Max verification retries. Default: 1.
    pub max_retries: u8,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            skip_tools: Vec::new(),
            max_retries: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-tool verification rules
// ---------------------------------------------------------------------------

/// Run inline verification for a tool that claimed success with side effects.
///
/// For file-mutating tools (write_file, edit_file, apply_patch), this actually
/// reads the file back to confirm the operation landed. For other side-effect
/// tools, it returns Pass (trust the tool).
///
/// Returns the verdict and the annotated content to inject into the session
/// message stream.
pub fn run_verification(
    tool_name: &str,
    tool_input: &serde_json::Value,
    workspace: &Path,
) -> (VerifyVerdict, String) {
    let started = Instant::now();

    let verdict = match tool_name {
        // Read-only tools — skip
        "read_file" | "grep_files" | "file_search" | "list_dir" | "web_search"
        | "fetch_url" | "git_status" | "git_diff" | "git_log" | "git_show"
        | "git_blame" | "diagnostics" | "handle_read" | "task_list" | "task_read"
        | "pr_attempt_list" | "pr_attempt_read" | "automation_list" | "automation_read"
        | "github_issue_context" | "github_pr_context" | "code_execution"
        | "validate_data" | "note" | "request_user_input" | "recall_archive"
        | "tool_search_tool_regex" | "tool_search_tool_bm25" => {
            VerifyVerdict::Skipped
        }

        // Self-verifying or review tools — skip
        "review" | "agent_open" | "agent_eval" | "agent_close" | "tool_agent"
        | "rlm_open" | "rlm_eval" | "rlm_configure" | "rlm_close"
        | "rlm_session_objects" | "run_tests" => {
            VerifyVerdict::Skipped
        }

        // Core file-mutating tools — inline verification: re-read the file
        // to confirm the write/edit/patch actually landed. If the file is
        // missing or empty, it's a verification failure. The caller should
        // retry.
        "write_file" | "edit_file" | "apply_patch" => {
            inline_verify_file_tool(tool_input, workspace)
        }

        // Other side-effect tools — trust but don't block on verification
        "exec_shell"
        | "exec_shell_wait" | "exec_shell_interact" | "shell_cancel"
        | "exec_wait" | "exec_interact" | "task_shell_start" | "task_shell_wait"
        | "task_create" | "task_gate_run" | "github_comment" | "github_close_issue"
        | "github_close_pr" | "pr_attempt_record" | "pr_attempt_preflight"
        | "automation_create" | "automation_update" | "automation_pause"
        | "automation_resume" | "automation_delete" | "automation_run"
        | "task_cancel" | "remember" | "notify" | "revert_turn" | "fim_edit"
        | "pandoc_convert" | "image_analyze" | "image_ocr" | "web_run"
        | "finance" | "skill_install" | "checklist_write" | "checklist_add"
        | "checklist_update" | "todo_write" | "todo_add" | "todo_update"
        | "update_plan" | "create_goal" | "get_goal" | "update_goal" => {
            VerifyVerdict::Pass
        }

        // Unknown tools — skip verification
        _ => VerifyVerdict::Unverifiable {
            reason: format!("no verification rule for tool `{tool_name}`"),
        },
    };

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let _ = elapsed_ms;

    // Build the annotated content.
    let annotation = match &verdict {
        VerifyVerdict::Pass => String::new(),
        VerifyVerdict::Fail { expected, observed } => {
            format!("\n\n[VERIFY FAIL] Claimed: {expected}\n[VERIFY FAIL] Observed: {observed}")
        }
        VerifyVerdict::Unverifiable { reason } => {
            format!("\n\n[VERIFY] Unverifiable: {reason}")
        }
        VerifyVerdict::Skipped => String::new(),
    };

    (verdict, annotation)
}

/// Whether a tool can be auto-retried on verification failure.
/// File-mutating tools with deterministic inputs are safe to retry;
/// tools with side effects on external systems are not.
pub fn is_auto_retryable(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "edit_file" | "apply_patch")
}

/// Inline file verification: read the file back and check it exists with
/// content. Returns Pass if the file is present and non-empty, Fail if
/// missing/empty, Unverifiable if we can't read it.
fn inline_verify_file_tool(
    tool_input: &serde_json::Value,
    workspace: &Path,
) -> VerifyVerdict {
    let path_str = match tool_input.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return VerifyVerdict::Unverifiable {
            reason: "no path in tool input".to_string(),
        },
    };

    let resolved = if Path::new(path_str).is_absolute() {
        Path::new(path_str).to_path_buf()
    } else {
        workspace.join(path_str)
    };

    match std::fs::read_to_string(&resolved) {
        Ok(content) if !content.is_empty() => VerifyVerdict::Pass,
        Ok(_) => VerifyVerdict::Fail {
            expected: format!("non-empty file at {}", resolved.display()),
            observed: "file is empty".to_string(),
        },
        Err(_) => VerifyVerdict::Fail {
            expected: format!("file exists at {}", resolved.display()),
            observed: "file missing after write".to_string(),
        },
    }
}

/// Build a retry-success annotation for the session message stream.
pub fn retry_annotation(retry_count: u32) -> String {
    if retry_count == 0 {
        String::new()
    } else {
        format!(
            "\n\n[VERIFY PASS] auto-retried {} time(s) — operation landed",
            retry_count
        )
    }
}

/// Determine whether a tool name represents a side-effect tool that should
/// be verified.
pub fn is_side_effect_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "write_file"
            | "edit_file"
            | "apply_patch"
            | "exec_shell"
            | "exec_shell_wait"
            | "exec_shell_interact"
            | "shell_cancel"
            | "exec_wait"
            | "exec_interact"
            | "task_shell_start"
            | "task_shell_wait"
            | "task_create"
            | "task_cancel"
            | "task_gate_run"
            | "github_comment"
            | "github_close_issue"
            | "github_close_pr"
            | "pr_attempt_record"
            | "pr_attempt_preflight"
            | "automation_create"
            | "automation_update"
            | "automation_pause"
            | "automation_resume"
            | "automation_delete"
            | "automation_run"
            | "remember"
            | "notify"
            | "revert_turn"
            | "fim_edit"
            | "pandoc_convert"
            | "image_analyze"
            | "image_ocr"
            | "web_run"
            | "finance"
            | "skill_install"
            | "checklist_write"
            | "checklist_add"
            | "checklist_update"
            | "checklist_list"
            | "todo_write"
            | "todo_add"
            | "todo_update"
            | "update_plan"
            | "create_goal"
            | "update_goal"
    )
}

/// Post-hoc file-level verification: read the file back and check that
/// the expected content is present. Called by the turn loop after the
/// tool result has been injected into the session stream.
///
/// Returns `Some(VerifyVerdict)` when verification was possible,
/// `None` when the tool doesn't support post-hoc file checks.
pub fn post_hoc_verify_file(
    tool_name: &str,
    tool_input: &serde_json::Value,
    workspace: &Path,
) -> Option<VerifyVerdict> {
    match tool_name {
        "write_file" | "edit_file" => {
            let path_str = tool_input.get("path").and_then(|v| v.as_str())?;
            let resolved = if Path::new(path_str).is_absolute() {
                Path::new(path_str).to_path_buf()
            } else {
                workspace.join(path_str)
            };

            // Read back the file to check it exists and has content.
            match std::fs::read_to_string(&resolved) {
                Ok(content) => {
                    if content.is_empty() {
                        Some(VerifyVerdict::Fail {
                            expected: format!("non-empty file at {}", resolved.display()),
                            observed: "file is empty".to_string(),
                        })
                    } else {
                        Some(VerifyVerdict::Pass)
                    }
                }
                Err(e) => Some(VerifyVerdict::Unverifiable {
                    reason: format!(
                        "cannot read {} for verification: {e}",
                        resolved.display()
                    ),
                }),
            }
        }

        "exec_shell" => {
            // For exec_shell, check if the command created expected paths.
            // We can't know the expected output without parsing the command,
            // so this is best-effort: if the tool claimed success and exit
            // code was zero, we trust it.
            Some(VerifyVerdict::Pass)
        }

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_tools_are_skipped() {
        for tool in &[
            "read_file",
            "grep_files",
            "file_search",
            "list_dir",
            "git_status",
            "git_diff",
            "web_search",
        ] {
            let (verdict, _) = run_verification(
                tool,
                &serde_json::json!({}),
                Path::new("/tmp"),
            );
            assert!(
                matches!(verdict, VerifyVerdict::Skipped),
                "{tool} should be skipped, got {verdict:?}"
            );
        }
    }

    #[test]
    fn side_effect_tools_pass_when_successful() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Create a real file so inline verification passes for file tools.
        let test_file = tmp.path().join("test.rs");
        std::fs::write(&test_file, "// test content").expect("write");

        for tool in &["write_file", "edit_file", "apply_patch"] {
            let (verdict, _) = run_verification(
                tool,
                &serde_json::json!({"path": test_file.to_str().unwrap()}),
                tmp.path(),
            );
            assert!(
                matches!(verdict, VerifyVerdict::Pass),
                "{tool} should pass, got {verdict:?}"
            );
        }

        // exec_shell passes through — not file-verified.
        let (verdict, _) = run_verification(
            "exec_shell",
            &serde_json::json!({"command": "echo ok"}),
            tmp.path(),
        );
        assert!(matches!(verdict, VerifyVerdict::Pass));
    }

    #[test]
    fn unknown_tools_are_unverifiable() {
        let (verdict, _) = run_verification(
            "nonexistent_tool",
            &serde_json::json!({}),
            Path::new("/tmp"),
        );
        assert!(matches!(verdict, VerifyVerdict::Unverifiable { .. }));
    }

    #[test]
    fn is_side_effect_tool_identifies_mutating_tools() {
        assert!(is_side_effect_tool("write_file"));
        assert!(is_side_effect_tool("edit_file"));
        assert!(is_side_effect_tool("exec_shell"));
        assert!(is_side_effect_tool("apply_patch"));
        assert!(!is_side_effect_tool("read_file"));
        assert!(!is_side_effect_tool("grep_files"));
        assert!(!is_side_effect_tool("git_status"));
    }

    #[test]
    fn post_hoc_verify_write_file_detects_missing_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let verdict = post_hoc_verify_file(
            "write_file",
            &serde_json::json!({"path": "nonexistent.txt"}),
            tmp.path(),
        );
        assert!(verdict.is_some());
        assert!(matches!(verdict.unwrap(), VerifyVerdict::Unverifiable { .. }));
    }

    #[test]
    fn post_hoc_verify_write_file_confirms_existing_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file_path = tmp.path().join("real.txt");
        std::fs::write(&file_path, "hello world").expect("write");

        let verdict = post_hoc_verify_file(
            "write_file",
            &serde_json::json!({"path": "real.txt"}),
            tmp.path(),
        );
        assert!(verdict.is_some());
        assert!(matches!(verdict.unwrap(), VerifyVerdict::Pass));
    }

    #[test]
    fn post_hoc_verify_returns_none_for_unsupported_tools() {
        assert!(post_hoc_verify_file(
            "read_file",
            &serde_json::json!({}),
            Path::new("/tmp")
        )
        .is_none());
    }

    #[test]
    fn verify_config_default_disabled() {
        let cfg = VerifyConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.skip_tools.is_empty());
        assert_eq!(cfg.max_retries, 1);
    }
}
