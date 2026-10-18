# CUR-002 — Stop interrupts and stays stopped

## Purpose

Prove that Stop interrupts a real working turn and the interrupted turn remains stopped after the chat view reloads, without replaying its original input.

## Covers

Plan task keys: no task key was captured for this legacy Stop subcase.

Defects: Stop reporting success without interrupting the own session, replay after reload, and a live child process being mistaken for a completed parent turn. Migrated from chat-send.md step 5.

## Tier

release-core

## Surfaces and platforms

- **Surfaces:** cli, web
- **Platforms:** linux, macos, windows
- **Providers:** claude, codex
- **Grants:** dev-fixture-spawn, owned-long-turn, provider-model-budget
- **Automation:** manual
- **Status:** active

## Preconditions and grants

Use an owned DEV tab and an explicitly bounded long turn. The owner grants the fixture spawn, model budget, and any child-process stop. Do not use a guessed delay or issue Stop before observing the owned working event.

## Fixture

Prepare a pad-authored long-turn task that exposes a real working event and can be interrupted without file or network actions. Record its tab, own provider session, turn ID, and initial message count.

## Steps

1. Start the owned turn once and wait for its actual working event.
2. Issue dc tab stop --tab TAB --json once. Require stopped=true and then observe the same session become idle.
3. Capture the persisted interrupted presentation, reload the view, and recount the original input. It must remain interrupted and appear once.
4. If a live child terminal may outlast the parent turn, compare parent status with turn-completed evidence. Stop the owned child only when its separate grant allows it.

## Oracle

PASS — Stop reports the observed interruption, the same session becomes idle, the interrupted turn remains visible after reload, and the original input is present once with no replay.

FAIL — Stop falsely reports success, the turn continues, the session changes, the input replays, or child activity is misreported as parent completion.

NOT RUN — no real working-event fixture, provider, model budget, DEV target, or required owner grant is available.

## Evidence to record

Record the case ID, provider, surface, UTC time, build SHA, boot ID, tab/session/turn/send IDs, exact Stop receipt, working-to-idle events, message counts before and after reload, and the opened UI/API evidence paths. Preserve parent and child process state separately.

## Cleanup

Stop only the owned child process when granted. Close the owned tab and workspace after evidence is saved. Do not replay the input or restart the provider session to make the view look idle.

## History of notable runs/defects

The source journey records a historical Claude interruption/restart/reload PASS. The comprehensive lifecycle ledger records busy Stop as PASS for Codex Chat and Claude PTY in api-stop-only.json and api-tool-stop.json; no task key was recorded for those cells. Internal subagent Stop and restored transcript 409 were open failures, not covered by the ordinary parent Stop result. No live run was made for this catalogue change.
