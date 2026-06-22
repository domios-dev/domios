# plugins-rethink Hardening — Test → Review → Test (pre-release)

> Owner: Dante (orchestrator). Brief from Andrey 2026-06-20. **Release is PARKED**
> until this completes AND Andrey's own tests pass. The `release-pipeline` task is
> removed from the queue for now.

**Goal.** Before any release: (0) fix the known chat-UI-default regression, (1)
establish a GREEN test baseline on BOTH macOS (here) and Windows (ROG), (2) run an
iterative Codex review→fix loop (~10 passes) across plugins-rethink to kill
legacy/patches/laziness/plugin smells against all the plans + capability
discipline, (3) re-run the FULL battery (local + ROG) to prove nothing regressed.
Fix everything we find — no shyness.

**Hard order (Andrey):** Phase 0 pre-fix → **Phase 1 TESTS (baseline)** → **Phase 2
REVIEW + FIXES** → **Phase 3 TESTS (regression)** → Phase 4 hand-off. Tests first,
then review/fixes, then tests again to confirm nothing broke.

**Cross-machine.** Every test phase runs in parallel on:
- **local** — macOS (this machine, dev :7686, the merge-space).
- **ROG** — Windows mesh peer (currently v1.4.5; must be rebuilt from plugins-rethink
  first). Coordinated via ROG's Main GA: `codeterm send --pane main --mesh ROG "…"`.
  Windows will surface platform-specific bugs (paths, PTY/ConPTY, keys, OSC7).

---

## Phase 0 — Known regressions (fix BEFORE the cycle)

- **P0.1 chat-UI-default regression** — IN PROGRESS (worker `ba973748`, Opus). The
  `default_new_agent_pane_chat` preference no longer auto-opens chat UI for PTY agent
  panes on new / restored / agent-created panes (only the FE atomic-spawn path applied
  it; backend restore + CLI-agent spawn never did). Likely T6.0 descriptor/atomic-spawn
  fallout. Fix all creation paths + TDD.
- **P0.2 ROG "only main tab works"** — investigate. ROG is on v1.4.5; first rebuild it
  from plugins-rethink (via Main GA), then confirm whether non-main panes work. If they
  still don't on the new build → a Windows-specific plugins-rethink regression → fix.

## Phase 1 — Test baseline (run FIRST, record everything)

Produce `BASELINE.md`: every suite → green / red(+output) / flaky / skipped(+why).
Known pre-existing failures listed explicitly so Phase 3 can diff. Run on local AND ROG.

**1a. Static gates / guards (fast):**
```
npm run check:versions && npm run check:plugin-versions
scripts/check-file-sizes.sh
scripts/check-prompts-doc.sh
scripts/check-api-docs.sh
scripts/check-blocking-in-async.sh
scripts/check-mux-write.sh
scripts/check-no-raw-graph-sql.sh
scripts/check-evals-list.sh
```
**1b. Rust:** `./build.sh test` (cargo test --lib + pty) + `cargo clippy --all-targets`
(the pre-push gate). Record counts.
**1c. Frontend unit:** `npm run test:unit`.
**1d. Plugins:** `npm run test:plugins` (claude/codex/cursor/opencode + lmstudio + others).
**1e. Types:** `./node_modules/.bin/tsc --noEmit` (clean in main checkout).
**1f. Playwright (dev up):** `npm run test:pw:lint` → `:smoke` → `:golden` → `:full`
against `./build.sh dev --wait`.
**1g. Evals (behavioural, the ones touching plugins-rethink):** `restore-matrix`
(cursor + providers), `session-restore`, `plugins`, `binding-matrix`, `cli-agent`
(model-judged, heavy), `mem-recall`/`dreaming` (if mem touched). Each per its README.
**1h. ROG:** rebuild from plugins-rethink, then run 1a–1f there (Windows). Use
`.\build.ps1` on Windows. Capture Windows-specific failures.

## Phase 2 — Codex review → fix loop (~10 passes)

**Technique (the "loop"):** each pass = (1) Codex reviews a SCOPED slice of
plugins-rethink against the plans (`docs/plans/*`) + `AGENTS.md` discipline, emitting
findings `{file:line, severity, category, why, suggested fix}`; (2) Dante triages
valid vs invalid (Andrey can veto/confirm); (3) valid findings → fix (Codex itself, or
a fanned-out worktree worker) → focused commit → targeted tests; (4) next pass.
Reviewer = **Codex** (Andrey's ask); fallback Opus/Sonnet when Codex is rate-limited.
Legacy deletion is encouraged (no-back-compat rule). No scope creep per finding.

**Scoped slices (one focus per pass):**
1. **Plugin boundary / capability discipline** — the 5 AGENTS.md rules
   (capabilities-not-identities; native+JS parity; detection-keys-off-own-session;
   host-API surface audit; version lockstep). Grep for leaked provider-id / literal
   `"codex"`/`"lmstudio"`/`kind ==` branches in core.
2. **Providers deep-dive** — claude/codex/cursor/opencode/lmstudio + **bitwarden**.
   Legacy, patches, laziness, error handling, sandbox holes. SPECIFIC: the bitwarden
   "expands a window/thing on load" smell → find the weird code, fix it.
3. **Core seams added in plugins-rethink** — ProviderRegistry/AgentTransport (T6.0),
   Messenger gateway, closed-history/restore + path-match, AS classifiers, chatbackend
   history, host.toolcall.parse.
4. **Async / hot-path discipline** — spawn_blocking on heavy/IO/subprocess in Tauri
   commands; daemon never shells out to its own CLI; lock ordering
   (mux→output_buf→screen→agent_tracker→pty); no nested `.lock()`.
5. **File-size + dead code + DRY/SOLID** — <1000 lines (mod.rs <800); allowlist must
   shrink, never grow; dead code; duplicated model/provider logic.
6. **Cross-platform** — Windows/macOS/Linux correctness in changed code (paths,
   `e.code` keys, PTY/ConPTY, OSC7). Cross-checked against ROG findings.
7. **Security / confinement** — host-API surface justification, secrets store, QuickJS
   sandbox, NO sudo (AS7).
8. **Test adequacy** — new behavior covered; no `#[ignore]` cheating; golden+negative
   corpora present; native/JS parity for every drop-in-able capability.
9. **Docs / plan drift** — each `docs/plans/*` vs actual implementation; revision tags
   bumped; API.md complete.
10. **Completeness critic** — adversarial sweep: what did passes 1–9 miss.

## Phase 3 — Regression (full battery again, local + ROG)

Re-run ALL of Phase 1 on both machines. Produce `REGRESSION.md` diffing vs
`BASELINE.md`: every previously-green suite still green; new fixes have passing tests;
playwright/E2E clean on both platforms. Any new red = block + fix before proceeding.

## Phase 4 — Hand-off (out of scope here)

Summarize baseline→fixes→regression deltas + residual known issues + readiness. Andrey
runs his personal tests and makes the release call. `release-pipeline` un-parks only then.

## Notes

- Reviewer model: Codex primary; Opus/Sonnet fallback on rate-limit.
- Parallelism: review passes are sequential (triage between); independent fixes fan out
  to worktree workers; local + ROG test runs are concurrent.
- **cursor self-use (experiment):** spawn cursor-agent in a terminal pane via the
  trust-prewrite + `cursor-agent --force --model auto` incantation; optionally use it as
  an extra reviewer/pair of hands during Phase 2.

<!-- revision: 1 -->
