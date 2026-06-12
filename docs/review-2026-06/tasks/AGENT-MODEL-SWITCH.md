# AGENT-MODEL-SWITCH — universal model switching with verified feedback

**Executor:** TBD (QUEUED — depends on DETECT-MODEL-FIX + SPAWN-MODEL-VALIDATE landing) · **Branch:** stabilization or follow-up · **Effort:** L (backend) + M (frontend)
**Backlog ref:** ../model-fidelity-report.md · user-requested 2026-06-12: "должно для всего работать… посмотреть че за модели и свичнуть, и когда нового агента создаешь тоже"

## Goal
`codeterm agent model <id> [--pane]` works for EVERY provider, verifies the switch actually happened, and reports the result; the UI exposes model choice at agent creation and a switcher on a running pane.

## Design (decided)
1. **Provider capability** `model_switch_strategy()`:
   - `SlashCommand(template)` — claude: send `/model {id}` raw (existing path).
   - `RestartWithModel` — codex/opencode: graceful in-pane restart reusing the `agent restart` machinery (/exit + relaunch) but with `model_launch_args(new_id)` injected and the SAME session resumed — conversation survives, model changes.
   - `Unsupported` — explicit error.
2. **Daemon-side op** (`agent.model` socket + REST): validate id against the provider's enumeration (SPAWN-MODEL-VALIDATE's source — same alias handling, same near-miss suggestion), execute the strategy, then **verify**: poll the pane's detected session model (DETECT-MODEL-FIX's now-trustworthy attribution) and/or structural TUI banner up to a timeout; return `{switched: true, model: <detected>}` or a loud failure. CLI prints the verdict — no more fire-and-forget.
3. **Frontend**: model dropdown in the new-agent dialog (fed by `agent providers`/`models`); clicking the model chip on an agent pane opens the same picker and calls the switch op; chip updates from the verified detection event.
4. Persisted: pane's requested model updates in the snapshot so restore relaunches with the NEW model.

## Dependencies (hard)
- DETECT-MODEL-FIX (verification oracle) — in flight.
- SPAWN-MODEL-VALIDATE (enumeration + validation source) — in flight.

## Split when dispatching
- AMS-1 backend: capability + daemon op + CLI verdict + unit tests (strategy table, validation reuse, verify timeout → failure).
- AMS-2 frontend: dialog dropdown + chip picker + event-driven chip update; component tests on the happy-dom harness.
- e2e (dev window): switch codex pane gpt-5.5 → gpt-5.4-mini, assert verified verdict + session continuity (split-marker message sent pre-switch is still in the transcript post-switch).

## Notes
- `agent restart` semantics already handle graceful exit + auto-resume; reuse, don't fork.
- DETECT-MODEL-FIX removed the codex/opencode `detect_active_session` roll-forward (it caused cross-pane contamination). Known limitation until this task lands: a manual in-TUI `/model` or `/new` in codex/opencode creates a session the pane no longer follows. The daemon-driven switch here is the structural replacement — it knows the new session deterministically.
- Docs: API.md, CLI.md (regen), SKILLS.md + skill folder, CONFIG.md if any default appears.

<!-- revision: 1 -->
