export default workflow({
  "id": "v0868-stopship-lane",
  "goal": "Verify the v0.8.68 Fleet, Workflow, Lane, Runtime, and gate receipt path without changing the workspace",
  "description": "Read-only release acceptance fixture for #4175, #4177, #4178, and #4179. Every Fleet role inspects checked-in runtime evidence; no step creates branches, edits files, installs dependencies, or publishes anything.",
  "gates": [
    {
      "id": "scout-evidence",
      "role": "scout",
      "on": "role_complete",
      "gate": "approve",
      "on_fail": "block",
      "blocks_role": "implementer",
      "max_retries": 0,
      "artifact_kind": "source_evidence"
    },
    {
      "id": "implementation-plan",
      "role": "implementer",
      "on": "role_complete",
      "gate": "approve",
      "on_fail": "block",
      "blocks_role": "reviewer",
      "max_retries": 0,
      "artifact_kind": "verification_plan"
    },
    {
      "id": "review-findings",
      "role": "reviewer",
      "on": "role_complete",
      "gate": "review",
      "on_fail": "block",
      "blocks_role": "verifier",
      "max_retries": 0,
      "artifact_kind": "review_report"
    },
    {
      "id": "verifier-evidence",
      "role": "verifier",
      "on": "role_complete",
      "gate": "verify",
      "on_fail": "block",
      "blocks_role": "release_lead",
      "max_retries": 0,
      "artifact_kind": "verification_report"
    }
  ],
  "nodes": [
    {
      "sequence": {
        "id": "acceptance-chain",
        "children": [
          {
            "agent": {
              "id": "scout-runtime",
              "prompt": "Read the checked-in v0.8.68 orchestration path only. Inspect workflows/v0868_stopship_lane.workflow.js, fleets/v0868-stopship.toml, crates/cli/src/lib.rs, crates/workflow/src/role_resolve.rs, and crates/tui/src/tools/workflow.rs. Report concise path-and-symbol evidence for: the stopship alias, named Fleet loading, role-to-profile resolution, tmux Lane launch, typed task_started receipts, gate_updated receipts, and terminal workflow receipts. Do not edit files, create branches, run shell commands, access GitHub, or infer success where source evidence is absent.",
              "agent_type": "explore",
              "role": "scout",
              "mode": "read_only",
              "file_scope": [
                "workflows/v0868_stopship_lane.workflow.js",
                "fleets/v0868-stopship.toml",
                "crates/cli/src/lib.rs",
                "crates/workflow/src/role_resolve.rs",
                "crates/tui/src/tools/workflow.rs"
              ],
              "budget": { "max_steps": 12, "timeout_secs": 600, "max_tokens": 6000 }
            }
          },
          {
            "agent": {
              "id": "plan-verification",
              "prompt": "Act as the Fleet implementer role for a verification-only acceptance run. Use the promoted scout source_evidence handoff to produce a no-edit verification plan for #4175/#4177/#4178/#4179. Identify the exact receipt fields that would prove role resolution, gate promotion or blocking, and terminal Lane reconciliation. This is deliberately not an implementation task: do not edit files, create branches, run shell commands, or propose fixes unrelated to missing acceptance evidence.",
              "agent_type": "implementer",
              "role": "implementer",
              "mode": "read_only",
              "file_scope": [
                "crates/cli/src/lib.rs",
                "crates/lane/src/registry.rs",
                "crates/tui/src/tools/workflow.rs"
              ],
              "budget": { "max_steps": 10, "timeout_secs": 600, "max_tokens": 5000 }
            }
          },
          {
            "agent": {
              "id": "review-contract",
              "prompt": "Review the promoted verification_plan handoff against the checked-in runtime. Look specifically for false-green risks: declared role versus resolved profile, gate state versus prose verdict, tmux process exit versus terminal workflow receipt, and a completed Lane with missing child evidence. Return APPROVE only when each claimed receipt has a concrete source owner; otherwise return BLOCK with the missing evidence. Remain read-only and do not run shell commands or edit anything.",
              "agent_type": "review",
              "role": "reviewer",
              "mode": "read_only",
              "file_scope": [
                "crates/cli/src/lib.rs",
                "crates/lane/src/registry.rs",
                "crates/tui/src/tools/workflow.rs"
              ],
              "budget": { "max_steps": 10, "timeout_secs": 600, "max_tokens": 5000 }
            }
          },
          {
            "agent": {
              "id": "verify-receipts",
              "prompt": "Statically verify the promoted review_report against existing tests and receipt serialization. Inspect the Workflow and CLI test modules for role-resolved task_started, gate_updated, run_completed, metadata, and Lane exit-receipt assertions. Return a compact PASS/BLOCK matrix with exact test names or source symbols. Do not run commands, edit files, create build artifacts, or treat this prose verdict as the gate state; the host gate is authoritative.",
              "agent_type": "verifier",
              "role": "verifier",
              "mode": "read_only",
              "file_scope": [
                "crates/cli/src/lib.rs",
                "crates/lane/src/registry.rs",
                "crates/tui/src/tools/workflow.rs"
              ],
              "budget": { "max_steps": 10, "timeout_secs": 600, "max_tokens": 5000 }
            }
          },
          {
            "agent": {
              "id": "release-receipt",
              "prompt": "Use the promoted verification_report handoff to produce the final acceptance receipt for #4175/#4177/#4178/#4179. List the declared Fleet role and resolved profile evidence, every gate state observed, the terminal workflow status required, and any blocker that prevents issue closure. Never claim that source inspection substitutes for a live Lane log. Do not edit, publish, close issues, run shell commands, or mutate the workspace.",
              "agent_type": "general",
              "role": "release_lead",
              "mode": "read_only",
              "file_scope": [
                "crates/cli/src/lib.rs",
                "crates/lane/src/registry.rs",
                "crates/tui/src/tools/workflow.rs"
              ],
              "budget": { "max_steps": 8, "timeout_secs": 600, "max_tokens": 4000 }
            }
          }
        ]
      }
    }
  ]
});
