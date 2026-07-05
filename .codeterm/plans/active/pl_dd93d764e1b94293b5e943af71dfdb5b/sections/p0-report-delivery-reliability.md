---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_c7b2661fa0e048b2bd5d7906f305c36a
handle: p0-report-delivery-reliability
title: P0 — Report delivery reliability before UX execution
---

Wave P0 (blocks dispatch of all UX implementation lanes t1-t7; runs immediately, parallel to nothing on this plan except planning itself): make Planner->Orchestrator report delivery loss-proof.

[Planner reconciliation note: this section was started by the Orchestrator during the incident; the Planner kept the handle and rewrote the body for coherence with the wave structure — correction recorded as a stride.]

Root cause (traced, confirmed): cli/src/cmd/agent.rs:349 calls report_guard::enrich_ready_report for readiness statuses; report_guard.rs::inspect_git_worktree treats ANY `git status --porcelain` output as lane dirt and returns readiness_dirty_error -> eprintln + exit(1) BEFORE the daemon call. Report is lost client-side. The daemon's own .codeterm/plans/** mirror writes make the main checkout dirty during planning, so refusal is near-deterministic there; the refusal text's tail is a dirty-file list that reads as git noise when output is piped/truncated.

Implementation areas:
- cli/src/cmd/report_guard.rs: exclude tool-owned paths (.codeterm/plans/** and daemon-written state) from dirt detection; split metadata enrichment (best-effort) from delivery gating; keep SHA-integrity refusal only for genuine lane files on readiness statuses.
- cli/src/cmd/agent.rs report arm: on refusal, do NOT exit before daemon contact — send a structured refusal/downgraded partial report (kind e.g. report_refused) so the parent sees a diagnostic; print REPORT NOT SENT as first AND last line with exact reason + next commands; print explicit report id on success (both text and --json paths).
- Daemon: src-tauri/src/agent/reports.rs — accept/store the structured refusal report kind; parent `codeterm agent reports` shows it.
- Prompts/docs: worker/orchestrator templates (src-tauri/prompts/) + docs/ORCHESTRATION.md teach: "no printed report id => not sent; rerun or use the fallback".
- Tests: cli unit tests beside report_guard (dirty tool-owned paths pass; dirty lane files refuse loudly with REPORT NOT SENT framing; partial/blocked never refused); daemon test for refusal-report storage/visibility.

Verification: single narrowed cargo test run per crate (cli + src-tauri); ./build.sh check; live dogfood smoke: from a dirty worktree, worker sends in_review -> parent receives intended report or structured refusal; `codeterm agent reports --pane <parent> --json` non-empty in both cases.
Approval gate: Orchestrator dispatches NO UX lane (t1-t7) until this task is done and the live smoke passes.
