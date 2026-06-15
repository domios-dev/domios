# MODEL-FIDELITY probe report (2026-06-12, prod live probe)

Verdict: `--model` plumbing is CORRECT end-to-end (CLI → REST → SpawnAgentParams →
provider `model_launch_args` → launch command; TUIs boot the requested model).
The lies are elsewhere: structural model DETECTION misattributes, and spawn does
not validate model ids — opencode silently falls back to MiMo on an unknown id.

## Probe table (requested → built cmd → evidence → result)

| # | Requested | Built command | TUI shows | pane JSON `agent_session_model` | Result |
|---|-----------|--------------|-----------|-------------------------------|--------|
| 1 | codex gpt-5.5 | `codex … -m gpt-5.5` | gpt-5.5 medium | null | DETECTION INCOMPLETE |
| 2 | codex gpt-5.4-mini | `codex … -m gpt-5.4-mini` | gpt-5.4-mini medium | `GPT-5.5` | **MISMATCH** |
| 3 | opencode opencode/big-pickle | `opencode -m opencode/big-pickle` | Big Pickle OpenCode Zen | `big-pickle` | MATCH |
| 4 | opencode opencode/deepseek-v4-flash | `opencode -m opencode/deepseek-v4-flash` | DeepSeek V4 Flash OpenCode Zen | `big-pickle` | **MISMATCH** |
| 5 | claude haiku | `claude … --model haiku` | Haiku 4.5 | null (session set) | DETECTION INCOMPLETE |

## Negatives (bogus `--model no-such-model-xyz`)

All three providers: spawn EXIT=0, pane created, no pre-spawn validation.
- codex: TUI boots claiming `model: no-such-model-xyz` (fails at first request).
- opencode: **silently boots MiMo V2.5 Free** — unknown id → default fallback.
  This explains the live mystery where a pane requested big-pickle-ish but showed
  MIMO: a wrong/partial model id (e.g. bare `big-pickle` instead of
  `opencode/big-pickle`) silently lands on the default free model.
- claude: TUI shows the bogus name (fails at first call).

## Detection plumbing (suspect sites)

- pane claims session + writes model: `src-tauri/src/service/pane.rs:224-345`
- codex `detect_session_model`: `src-tauri/src/plugin/agent_codex/provider.rs:185-190` (session file)
- opencode: `src-tauri/src/plugin/agent_opencode/provider.rs:485-488` — reads
  **DB latest assistant model** (global latest, not pane-own session → cross-pane
  contamination is the likely mismatch mechanism)
- claude: `src-tauri/src/plugin/agent_claude/provider/mod.rs:165-167`, `sessions.rs:58-65`

## Discoverability

- `agent spawn --help` is good (explains `--model` + `agent models <provider>` discovery).
- Skill folder had PHANTOM commands, now fixed on disk (SKILL.md rev 42):
  `codeterm agent status` (real surface: `pane status --pane ID`),
  `codeterm browser open` (no such command; web surface is `web link`),
  stale Lead-role guidance (roles are Orchestrator/Worker only).

## Spawned follow-up tasks

- DETECT-MODEL-FIX — detection must attribute the pane's OWN session model.
- SPAWN-MODEL-VALIDATE — validate model id at spawn where enumerable; loud failure.

<!-- revision: 1 -->
