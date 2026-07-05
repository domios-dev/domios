---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_15f461151cfa4a32949e8f888e0498c3
handle: p0-report-reliability
title: 'P0: report delivery reliability (no silent loss)'
status: done
---

Make agent report delivery loss-proof. This task BLOCKS dispatch of all UX lanes (t1..t7); only planning and wave-0 lanes may run alongside it.

Root cause (already traced — verify, do not re-discover): cli/src/cmd/agent.rs:349 -> report_guard::enrich_ready_report; report_guard.rs::inspect_git_worktree treats ANY dirty `git status --porcelain` line as lane dirt for readiness statuses (in_review/done) and errors out with eprintln+exit(1) BEFORE the daemon call. The daemon's own .codeterm/plans/** mirror writes keep the main checkout dirty during planning, so refusal is near-deterministic there. Refusal text tail is a dirty-file list -> reads as git noise in piped output; worker believes report sent; `codeterm agent reports --pane <parent>` returns [].

Steps:
1. report_guard.rs: exclude tool-owned paths (.codeterm/plans/**, other daemon-written state under .codeterm/) from dirt detection; separate metadata enrichment (best-effort, never fatal) from delivery gating; keep SHA-mismatch refusal only for genuine lane files.
2. agent.rs report arm: on refusal, still contact the daemon with a structured refusal/downgraded report (e.g. kind=report_refused, original message + reason attached) so the parent gets a visible diagnostic; CLI output on refusal: first AND last line literally "REPORT NOT SENT", reason + exact next commands between; non-zero exit; --json carries the same verdict. On success print the explicit report id (text + json).
3. Daemon src-tauri/src/agent/reports.rs: accept/store the refusal kind; `codeterm agent reports` lists it.
4. Guard invariants with tests: partial/blocked NEVER refused; dirty tool-owned paths pass readiness; dirty lane files refuse loudly with the REPORT NOT SENT framing; success prints report id.
5. Prompts/docs: src-tauri/prompts worker/orchestrator templates + docs/ORCHESTRATION.md: "no printed report id => not sent"; document the fallback (codeterm send to parent pane).
6. Live dogfood smoke (gate): from a deliberately dirty worktree, send --status in_review; parent's `codeterm agent reports --pane <parent> --json` shows the intended report or the structured refusal — never [].

Commands: narrowed cargo test per crate (cli, src-tauri), one at a time; ./build.sh check; ./build.sh cli; the live smoke above against dev (7686).
Report: root-cause confirmation, diff summary per area, test output, live smoke transcript. Given the irony: commit your lane before reporting in_review, and if no report id prints, use codeterm send to the orchestrator pane.
