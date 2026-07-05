---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3aecfcdf6f5648da90ad9bf876db1a79
handle: chat-status-diverges-from-tui-truth-structural-fingerprints-not-verb-lis
title: 'Chat status diverges from TUI truth: structural fingerprints, not verb lists'
status: done
---

## What

BUG (round-33, user screenshots): while an agent worked, the CHAT view's status line showed a wrong/stale state while the TERMINAL (TUI) showed the truth. Evidence pair: chat shows the session ending at "Crunched for 53s" with commands rendered but no live state; the console simultaneously shows the real live footer "Inferring… (58s · ↓797 tokens · thinking)". The chat status is derived, and its derivation broke.

## Prime suspect (verify first)

Provider status detection. The architecture rule (AGENTS.md): detection uses STRUCTURAL fingerprints, never literal strings — vendors change copy without notice. Claude Code's current builds cycle whimsical spinner verbs ("Crunched", "Brewed", "Inferring", …). If the chat-status parser (src/components/ChatView/parser.ts or the Rust-side status classifier feeding chat state) matches on a verb list or specific phrases, new verbs silently fall through — status freezes at the last recognized state. GREP for literal spinner-verb matching in both TS and Rust status paths; that is the violation if present.

## Fix requirements

1. Status derives from structure (spinner glyph/frame pattern, footer position, token counter presence, elapsed-time pattern), never from verb dictionaries. If a dictionary exists, delete it.
2. Fixture-locked: capture the CURRENT prod Claude Code footer output (the exact bytes around "Inferring… (58s · ↓797 tokens · thinking)") as a parser fixture + one older-format fixture; parser tests assert the derived status on both generations (codex/opencode fixtures too if they share the path).
3. Chat view and pane-status tracker must agree: one derivation source for "agent is working" consumed by both (if they compute independently today, that is a second bug — unify or add a parity test).
4. Honest degradation: an UNRECOGNIZED footer shows a neutral working state (spinner present = working), never a frozen stale status.

## Executable test plan

1. Unit: parser fixtures (current-gen + legacy + codex) -> expected status matrix; new-verb fixture ("Xyzzying… (2s)") -> still classified working via structure.
2. Live dev: start an agent, compare chat status vs TUI footer at 5s intervals for 60s (script or manual transcript) — zero divergence windows > 2s.
3. Regression: the existing chat-ui parser suite green.
