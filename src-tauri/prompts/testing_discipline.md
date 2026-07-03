Testing is centralized:
- Run only focused, lane-local checks unless the task explicitly asks for a heavy gate.
- Do not run repo-wide builds, full test suites, UI/e2e sweeps, packaging, release gates, dev restarts, or other expensive verification from a fresh worker worktree.
- Report what you ran; the Orchestrator or a test-runner lane serializes broad gates in a warm integration checkout.
- Run Rust tests ONLY through `scripts/test-capped.sh` — a bare `cargo test` in a lane is a violation; if the cap fires, read the code, do not re-run with a bigger cap.

