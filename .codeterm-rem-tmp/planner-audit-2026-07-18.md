# Planner audit — 2026-07-18 (Curie, pane 02d1339c)

## 0. P0 DATA-INTEGRITY FLAG — read this first

**pl_ghk358 ("Automatic PTY delivery guarantee") is GONE from the plan store.**
Not in `.codeterm/plans/active/`, not in `archive/`, `plan list --all` does not know it,
every `task show` for its 39 tasks errors. The store was migrated to a new layout
(pl_as7ty5 now has `attachments/ handoffs/ sections/` subdirs) between my last write
(~12:30 revision strides) and now; the migration or the restart dropped an ACTIVE plan.

- The WORK survives: git lanes exist (`lane/fix-mesh-fail-closed`, `lane/fix-crash-recovery-state`,
  `lane/fix-mem-cli-transport`, `lane/research-chat-binding-crash`, report-card lane b03065045),
  and the worker ledger for tab 166df18d still lists all seven pl_ghk358 lanes.
- The PLAN entity is lost: spec (incl. sec_gnk2xm live-defect fixtures, sec_0gi9oc acceptance
  model, sec_sve813 corrected #240 premise), 39 task briefs, revision strides on task_eee5uy.
- **My session context is currently the only complete copy.** I can reconstruct the plan
  verbatim on request (new ids, same content). Decide: (a) reconstruct as new plan,
  (b) consolidate remaining open work into pl_as7ty5, (c) recover from a daemon-side
  backup if the migration wrote one. If this was NOT an intentional consolidation,
  file it as a new P0 issue: plan-storage migration drops active plans.

## 1. The 201-report ledger — cannot audit from here

`codeterm agent reports --tab 166df18d` returns empty from my pane; no 201-entry store
is reachable, and the plan that held report milestones is gone (see §0). To run the
duplicate/stale vs implemented+ungated vs truly-open triage I need either the ledger's
location/dump or read access from this pane. Agreed regardless: **nothing closes from
report count** — closure stays evidence-only, per the original closure-gate contract
(red-green regression or before/after transcript, Orchestrator closes).

## 2. Chat invariant review (task_wf0smk, pl_as7ty5) — planner verdict

Invariant as stated: once `chat_start` attaches (or a chat event arrives), the Chat
surface is authoritative — no late boot/loading/failure overlay may cover it or
interrupt the composer; retry is pane-generic (`restart_agent_pane`), never the
GA-only route; real pre-attach launch failures still surface; Retry hidden when
`canRetry=false`.

**Sound, and it is the frontend twin of the merged backend fix (task_i3xzn5 /
BootStage discipline, deployed in 31f335d48).** Backend now refuses to EMIT stale
SpawnFailed over a live PTY; wf0smk makes the frontend refuse to HONOR one that
arrives anyway. Complementary, not duplicate. Three planner notes:

1. **Cover PtyReattached explicitly.** The merged i3xzn5 fix introduced
   `BootStage::PtyReattached`, emitted only over a stale SpawnFailed. The invariant's
   test matrix should include: attached chat + late `pty_reattached` event → no
   overlay, no composer interruption, failure chip clears.
2. **"Chat event arrives" needs an ordering definition** vs session binding: an event
   from a PREVIOUS binding epoch must not count as attachment proof (own-session rule,
   architecture rule 3). The lost binding research (task_2si5jz report) traced these
   epochs — another reason to settle §0 recovery.
3. **The invariant is one-directional and should stay so:** pre-attach real launch
   failures keep their overlay (task text already preserves this). Do not let the fix
   suppress SpawnFailed for panes that never attached chat.

Criteria/test-plan quality: good — role/name selectors, no sleeps, real PTY Codex
pane, caps present, GA-route exclusion asserted. Dispatch-ready as written with note 1
added to the matrix.

## 3. Remaining-gates table (from session knowledge + worker ledger + git)

| Item (orig. task) | Lane / SHA | State | Exact remaining gate → merge action |
|---|---|---|---|
| #240 core taxonomy (task_7fei9d) | lane/fix-mesh-fail-closed @ c2b73fb10 (fix 04498de10), pushed | in_review | (1) `cargo clippy --all-targets` warm checkout (new SendRejection/axum surface, await_holding_lock DENY); (2) two-instance live drill: busy pane → `pane_busy_refused` + far-end PTY untouched; nonexistent → `pane_not_reachable_on_peer`; valid → parity with `--tab main`, assert target bytes; (3) warm full capped Rust + PW golden; then real merge + `merge-base --is-ancestor c2b73fb10` |
| Crash classify (task_4lpla2) | lane/fix-crash-recovery-state (pane debf4708) | in_review | (1) confirm red-run capture exists; (2) live drill: `kill -9` dev with 2-role team, restart, no session warning after ready AND after UI refresh; (3) warm full suite; real merge + ancestor check |
| Report cards (task_qfcx4i) | lane @ b03065045, pushed green | blocked (gate only) | merge lane into integration, then from canonical: `npx playwright test <spec> -g "report card body" --timeout=30000 --global-timeout=600000` ALONE — this is the sole remaining gate |
| #239 mem transport (task_s5ja8t) | lane/fix-mem-cli-transport | blocked (decision) | Orchestrator/owner decision: re-scope to diagnostics-only (criterion 2 already landed) + separate daemon socket-mem design task, or park |
| Codex submit repro (task_8e3uyw) | ledger lane ba3c1ac5 "Fable: Codex submit repro", active | in flight | mechanism verdict (does optimistic 9dbcda652 disarm the Enter retry at MAX_ENTER_ATTEMPTS loop) → bounded enter-retry w/o veto revert → red-green + 3x codex drill |
| Acceptance seam (task_8w2lg2) | — | pending | dispatch AFTER 8e3uyw verdict; session-store oracle + delivery_state + acceptance matrix is the goal-closing gate |
| Spawn factory (task_f784fk) | origin/lane/spawn-factory @ b11e61ab | pending | merge + ancestor check + POST-MERGE bidirectional intro drill on merged head |
| Gateway ph1 (task_by7ta6) | origin/lane/message-gateway @ 7dc7dcd6 | blocked by 7fei9d merge | refresh lane by merging target in, parity tests incl. #240 error cases, merge + ancestor |
| Board team-scope (task_aatfdq) | origin/lane/board-team-scope @ cd1613dff | blocked (bounce record) | needs the written bounce reasons before any rework |
| Chat invariant (task_wf0smk, pl_as7ty5) | — | pending | dispatch-ready; add PtyReattached case to matrix (see §2) |

Closure-gate discipline unchanged: every GitHub issue closes only on accepted
implementation + captured evidence; report count is never evidence.
