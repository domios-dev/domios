# Stabilization Backlog — June 2026 Consistency Audit

**Status:** OPEN — this is a task backlog, not a report. Each entry is a self-contained
assignment for an executor agent. The arbiter (orchestrator) dispatches tasks from here;
executors do NOT pick tasks themselves.

**How this was produced:** six parallel read-only audit passes (docs↔code, prompts↔code,
testing infra, TS duplication, Rust duplication, agent tooling/hygiene) on branch
`security` @ 2026-06-11, followed by spot-verification of every HIGH finding by the
arbiter. Findings the verification *contradicted* are phrased as "reconcile" tasks, not
"fix" tasks.

**Second pass (same day):** an adversarial self-review re-verified the HIGH findings
directly and recovered topics the audit passes dropped. Material corrections: the
file-size gate is broken at HEAD, not just in the working tree (A5 rewritten, now P0);
git hooks are not installed at all (new A6); `service/` vs `services/` duplication
(new F4) and the error-handling sweep (new F5) were silently dropped by the Rust
auditor; WS-G now exists in this backlog instead of only in agent-team.md; F1 evidence
upgraded from "audit claim" to verified, with a third occurrence (cookie Max-Age).

**Third pass (same day):** every remaining audit-sourced claim was verified against
code. One task WITHDRAWN: B6 — the PROMPTS.md "stale refs" finding was false (the doc
is correct; only the 3-way drift check survives, inside B7). A5's violation table was
itself incomplete (a `tail` cut it): the real list is 12 files, adding
`cli/src/cmd/plugin.rs` (1743 — largest CLI file, missed by every prior pass),
`agent/provider.rs` (1231), and both TSX god-files. B2 narrowed: dispatch is
verifiably wired, only runtime behavior + wording remain. B3/B5/D1/D3/D4 evidence
upgraded from "audit claim" to verified. New: A7 (supply-chain audit — no dependabot,
no cargo/pnpm audit anywhere), F6 (plugin.rs split), issue templates folded into D9.

**Companion docs:**
- [testing-process.md](testing-process.md) — the target testing methodology (WS-D implements it)
- [agent-team.md](agent-team.md) — multi-agent onboarding + model-routing rubric
- [tasks/](tasks/README.md) — **dispatch protocol**: per-task brief files in
  idiot-proof format (Evidence → Steps → Verify → Report). Workers execute the
  brief file, not this backlog. Briefs are written by the arbiter per wave; the
  backlog entry is the rationale, the brief is the executable.

---

## Rules for executors (read before any task)

1. **Verify first.** Every task lists evidence (file:line). The audit was automated —
   re-confirm the evidence still holds before changing anything. If it doesn't, report
   back "evidence stale" instead of improvising.
2. **Don't break the idea.** The goal is to stabilize and develop the product, never to
   rip out intent. If a task seems to conflict with VISION.md or CLAUDE.md hard rules,
   stop and report.
3. **One task = one branch/commit scope.** No drive-by fixes outside the task's
   acceptance criteria.
4. **Acceptance criteria are the definition of done.** Run the listed verification
   commands and paste their output in your report.
5. **Docs touched → bump `<!-- revision: N -->`.**
6. Never commit/push without explicit user request (CLAUDE.md hard rule).

**Fourth pass (2026-06-11, live prod probe):** the arbiter (Lamarr, pane `5c64a651`,
Orchestrator inside prod CodeTerm :7685) verified the control handles against the
running binary. Results: spawn works for all three providers (claude/codex/opencode
probes round-tripped spawn → task → send-text reply → report), `--model` selection
works (G3 RESOLVED), invalid roles are silently swallowed (C1 upgraded: `--role Lead`
→ exit 0, stored `role=None`), `workspace create --cwd` has a live bug (new F7), and
the shipped `codeterm` skill contradicts the binary (new B10). Wave 0 added: finish
and land branch `security` before stabilization work starts.

**Severity:** P0 = broken right now (CI red / security) · P1 = actively misleading
agents/users · P2 = debt, fix when adjacent · P3 = nice-to-have.

**Effort:** S (<1h agent time) · M (half-day) · L (multi-session, needs plan).

**Model:** recommended executor tier — see [agent-team.md](agent-team.md) for the rubric.

---

## Wave 0 — finish & land branch `security` (gates everything else)

**Executor: Opus pane (`232c0b4a`) — single agent, sequential. The working tree at
`C:\Developer\codeterm` is shared between all panes; parallel editing of tracked
files is forbidden until this lands.**

### W0 — Complete, verify, commit, and push the in-flight `security` work
**P0 · Effort: L**

**State (verified 2026-06-11).** Branch `security`: 39 modified tracked files +
3 untracked work files, uncommitted. One local commit unpushed (ahead of
`origin/security` by 1; ahead of `origin/develop` by 6). The dirty files form two
strands:
1. **Chat-parsing consolidation** (plan: `docs/superpowers/plans/
   2026-06-10-chat-parsing-dry-consolidation.md`, untracked — commit it too):
   `packages/chat-core/src/markers.ts` (+ untracked `markers.test.ts`),
   `packages/chat-ui/*` (MessageBubble, CodetermCard, templates, recognize.test),
   `plugins/provider-{claude,codex,opencode}/*`, `src-tauri/src/agent/chat_types.rs`,
   `src-tauri/src/service/pane_chat/*`, `src/components/Terminal/PaneSessionChat.tsx`,
   untracked `tests/playwright/suites/03-agents/cross-pane-chat.spec.ts`, plan
   `2026-06-10-crosspane-render-fixes.md`.
2. **Security hardening**: `src-tauri/src/api/{auth,web_routes,routes}.rs`,
   `mesh_routes/*`, `secrets/keychain.rs`, `services/tokens.rs`, plus
   `commands/pane.rs`, `config/schema/mod.rs`, `session/store.rs`,
   `startup/servers.rs`, `multiplexer/state/*`, provider/parity files.

**Task.**
1. Read both plan docs; determine completion state of each phase (the consolidation
   plan's own status notes claim "implemented, cleanup pending").
2. Finish what's actually unfinished — no new scope; if a phase is genuinely large,
   report back instead of improvising.
3. Verify: `./build.sh check`, `pnpm test:plugins`, narrow `cargo test` for touched
   Rust modules (PowerShell foreground, ≤90s, narrow filters), the new cross-pane
   Playwright spec against dev (7686) if dev is up.
4. **CI reality**: the file-sizes workflow will fail any PR/push from this branch
   (12 non-allowlisted >1000-line files — see A5 table). Include A5's baselining as
   its own commit (`scripts/file-size-allowlist.txt` additions + the `>800` comment
   fix in the workflow) so the branch can go green. This pre-executes A5; mark it.
5. Commit in logical chunks (consolidation / security / allowlist), English messages,
   scope prefixes, no co-author lines. **Do NOT commit:** `docs/review-2026-06/`
   (belongs to the stabilization branch), `chat.json`, `sp.json`.
6. Push `security`; verify CI green; report.
7. After arbiter confirmation: merge to `develop` (user pre-authorized merge
   conditional on green checks), push, then create branch `stabilization` from
   `develop`.

**Acceptance.** `git status` clean except the three excluded paths; CI green on
`security`; pasted check/test output; report delivered to pane `5c64a651`.

---

## WS-A — Repo hygiene & CI truth (P0/P1, do first)

### A1 — Split `dreaming.rs`: the file-size CI gate is RED right now
**P0 · Rust · Model: Opus · Effort: L**

**Problem.** `src-tauri/src/mem/dreaming.rs` is 1492 lines vs allowlist baseline 1241
(+251). Verified by running `scripts/check-file-sizes.sh` locally; the CI workflow
(`.github/workflows/file-sizes.yml`) runs the script in `--fail` mode on every
source-touching PR and on pushes to develop/master, so this regression alone blocks
CI. (Eight MORE non-allowlisted files are also over the limit at HEAD — that broader
triage is A5; this task is only the dreaming.rs split.)

**Task.**
1. Read `src-tauri/src/mem/dreaming.rs`, map responsibilities (schedule management,
   REM/dream stage orchestration, prompt assembly, session manipulation are the
   expected seams per the audit).
2. Split by responsibility into a `dreaming/` module dir; keep `mod.rs` < 600 lines
   (facade + glue per CLAUDE.md). Move any inline `#[cfg(test)] mod tests` body to
   `dreaming/tests.rs`, keeping `mod tests;` in source.
3. Remove `dreaming.rs` from `scripts/file-size-allowlist.txt` once every resulting
   file is < 1000 lines.
4. No behavior change: this is a mechanical move-and-re-export refactor.

**Acceptance.** `bash scripts/check-file-sizes.sh` exits 0; `./build.sh check` passes;
`cargo test mem::dreaming` (PowerShell foreground, ≤90s timeout) passes; no public API
of the `mem` module changed (grep callers unchanged).

**Risks.** dreaming.rs drives the Dreamer pipeline (prompts at
`src-tauri/prompts/dreaming_rem.md` reference its stages). Do not rename CLI/REST
surfaces. parking_lot lock ordering must survive the move — grep nested `.lock()` after.

---

### A2 — Untrack committed binaries, screenshots, and root junk
**P1 · hygiene · Model: Sonnet · Effort: S**

**Problem.** Verified via `git ls-files`: tracked at repo root —
`codeterm-cli-macos-aarch64` (4.7M), `codeterm-cli-macos-x86_64` (4.9M),
`.tmp-burger-a.png`, `.tmp-burger-a-collapsed.png`, `.tmp-burger-a-refined.png`,
`.tmp-final-ui.png`, `.tmp-sidebar-tab-refined.png` (~610K total), `e2e_proof.txt`.
Untracked noise: `chat.json`, `sp.json`, `NUL` (Windows reserved name). ~9.6MB of
platform-specific binaries in every clone; binaries drift from source since `build.sh`
is the build source of truth.

**Task.**
1. `git rm --cached` the two binaries, the five `.tmp-*.png`, `e2e_proof.txt`.
   Do NOT rewrite history (not worth the disruption for a personal repo; note in the
   commit message that blobs remain in history).
2. Extend `.gitignore`: `codeterm-cli-macos-*`, `.tmp-*.png`, `chat.json`, `sp.json`,
   `e2e_proof.txt`, `NUL` (verify which patterns already exist first — some do, but
   were added after the files were indexed).
3. Delete the local `NUL` files (root and `cli/NUL`) — Windows reserved filename,
   created accidentally by a `> NUL` redirect in a non-cmd shell. Also delete the
   stray `.qa-read.py` at root (untracked one-off, verified not in git).
4. Add one line to `docs/contributing/style.md`: design artifacts (screenshots/videos)
   go in PR descriptions, never the source tree; release binaries come from CI only.

**Acceptance.** `git ls-files | grep -E "codeterm-cli-macos|\.tmp-.*\.png|e2e_proof"`
empty; `git status` clean of the noise files; clone-size note in commit message.

**Risks.** Confirm nothing references the committed binaries (grep `codeterm-cli-macos`
in scripts/, docs/, .github/) before removal — release workflows must build their own.

---

### A3 — Consolidate `plan.md` / `PLAN_V2.md` into `docs/plans/`
**P2 · docs · Model: Sonnet · Effort: S**

**Problem.** Three competing plan locations: root `plan.md` (tracked), root
`PLAN_V2.md` (tracked), and the canonical `docs/PLANS.md` index over `docs/plans/*.md`
(~20 plan files). New contributors/agents can't tell which is live.

**Task.** Read both root files. Anything still relevant → migrate into a
`docs/plans/<topic>.md` and index in `docs/PLANS.md`. Everything else → delete (git
history keeps them; CLAUDE.md: "Delete legacy aggressively"). Add a line to
`docs/PLANS.md` header: "Plans live here and only here."

**Acceptance.** Root has no `plan.md`/`PLAN_V2.md`; `docs/PLANS.md` indexes anything
migrated; revision tag bumped.

---

### A4 — Stand up lint enforcement (rustfmt/clippy + TS lint)
**P1 · CI · Model: Codex Spark (config) + Sonnet (wiring) · Effort: M**

**Problem.** Verified: no `rustfmt.toml`, no clippy/fmt in `.githooks/pre-push` (only
`cargo check`) or any workflow; no eslint/biome config anywhere; root `package.json`
has no `lint` script. CLAUDE.md mandates style discipline but nothing enforces it —
and non-Claude agents (Codex workers) will format to their own defaults.

**Task.**
1. Rust: add `rustfmt.toml` (default style), run `cargo fmt` once as an isolated
   commit ("fmt-only, no logic"), then add `cargo fmt --check` + `cargo clippy
   --workspace --all-targets -- -D warnings` to `.githooks/pre-push` and a CI workflow.
   If clippy explodes with hundreds of pre-existing warnings, gate with
   `-A` on the noisy lints first and ratchet later — document the ratchet list.
2. TS: add Biome (single tool, fast, no plugin zoo — preferred over eslint+prettier
   here) at root with a minimal config; `"lint": "biome check ."` in root
   `package.json`; wire into CI.
3. Update `docs/contributing/style.md` to reference both configs.

**Acceptance.** Fresh clone: `cargo fmt --check` exit 0, `pnpm lint` exit 0, CI
workflow file exists and runs both on PRs.

**Risks.** The one-time `cargo fmt` diff is large — it MUST be a standalone commit with
nothing else in it, or it poisons `git blame` review forever. Coordinate with in-flight
branches (this repo has a dirty `security` branch right now — land after it merges).

---

### A5 — Drain the file-size gate: eight non-allowlisted violations sit in HEAD
**P0 · CI · Model: Sonnet (triage) · Effort: M**

**Problem.** Resolved during the second pass — the script is NOT lenient. It has two
modes: `--warn` (default, exit 0 — what a bare local run shows) and `--fail`; the CI
workflow runs `--fail`, which rejects ANY non-allowlisted file >1000 lines. Verified
at HEAD (not working-tree noise — `git show HEAD:<file> | wc -l`):

Complete violation list (third pass, full `--warn` output — an earlier `tail` had cut
it to 8 files; line counts are working-tree, spot-checked HEAD values in parens where
they differ):

| File | Lines |
|---|---|
| `packages/chat-ui/src/MessageBubble.tsx` | 2216 (→ E3) |
| `cli/src/cmd/plugin.rs` | 1743 (→ F6 — missed by every audit pass) |
| `src-tauri/src/plugin/js_runtime/parity_test.rs` | 1499 (1311 @HEAD) |
| `src-tauri/src/plugin/js_runtime/provider.rs` | 1394 |
| `src-tauri/src/mesh/mod.rs` | 1276 (→ F2) |
| `src-tauri/src/agent/provider.rs` | 1231 |
| `src-tauri/src/mem/store/tests.rs` | 1215 |
| `src/components/Terminal/PaneSessionChat.tsx` | 1152 (→ E3) |
| `src-tauri/src/mem/search.rs` | 1137 |
| `src-tauri/src/service/pty_io.rs` | 1071 (1048 @HEAD) |
| `src-tauri/src/plugin/agent_opencode/provider.rs` | 1046 |
| `src-tauri/src/mem/graph_repair/tests.rs` | 1040 |

Only `dreaming.rs` is allowlisted. Conclusion: file-sizes CI cannot pass on any
source-touching PR right now; these files reached HEAD either past red CI or without
PRs — also worth checking whether master/develop branch protection actually requires
the workflow (`gh api repos/{owner}/{repo}/branches/master/protection`); if not,
gating is advisory regardless of workflow color.

Bonus drift: the workflow comment says "Legacy **>800** files are baselined" while the
script's limit is 1000 — fix the comment.

**Task.**
1. Confirm the table against current HEAD (files move fast on this branch).
2. For each violator decide with the arbiter: split now (own tasks exist — F2
   mesh/mod.rs, F6 plugin.rs, E3 both TSX files) vs baseline into
   `file-size-allowlist.txt` at the HEAD count (graceful deprecation, per the
   allowlist's own header). Default: baseline everything, including files with split
   tasks (the split then removes the entry), so CI goes green TODAY and the ratchet
   does its job.
3. Fix the `>800` comment in `file-sizes.yml`.
4. Report whether branch protection actually requires this workflow (see above).

**Acceptance.** `bash scripts/check-file-sizes.sh --fail` exits 0 at HEAD (after A1
or with dreaming.rs re-baselined per arbiter decision); every allowlist addition has
a one-line justification comment; workflow comment matches script reality.

**Risks.** Re-baselining dreaming.rs instead of splitting (A1) is the arbiter's call —
doing both wastes nothing, but don't let A5 silently absorb A1's split.

---

### A6 — Git hooks exist but are not installed anywhere
**P1 · CI/dev-env · Model: Sonnet · Effort: S**

**Problem.** Verified: `.githooks/pre-push` exists (cargo check gate), but
`git config core.hooksPath` on the primary dev machine points at the default
`.git/hooks` (containing only `.sample` files). Nothing in `build.sh`, docs, or CI
sets `core.hooksPath .githooks` — so the pre-push check runs for nobody. A hook that
isn't installed is documentation cosplaying as enforcement.

**Task.**
1. Wire `git config core.hooksPath .githooks` into the natural bootstrap point —
   `build.sh` (first run of `dev`/`check` is fine; idempotent, one line) — and
   document it in `docs/BUILD.md`.
2. Audit `.githooks/` contents: what does pre-push actually run, how long does it
   take, does it respect the Windows/PowerShell constraints from CLAUDE.md? Anything
   slower than ~30s belongs in CI (D7), not a hook.
3. Coordinate with A4: fmt/clippy go into the hook only if fast; otherwise CI-only.

**Acceptance.** Fresh clone + `./build.sh check` → `git config core.hooksPath`
returns `.githooks`; a push with a compile error is rejected locally (demonstrate
once, paste output).

---

### A7 — Supply-chain hygiene: no dependency auditing exists at all
**P2 · CI/security · Model: Sonnet · Effort: S**

**Problem.** Verified third pass: no `.github/dependabot.yml`, no `cargo audit`/
`cargo deny`, no `pnpm audit` anywhere in workflows or build.sh. The dependency tree
(Tauri + axum + a JS workspace + QuickJS-hosted plugins) is large and
security-relevant (this branch IS a security branch), and vulnerable-dependency
discovery currently relies on nobody.

**Task.**
1. Add `dependabot.yml` covering `cargo`, `npm` (pnpm), and `github-actions`
   ecosystems, weekly, with grouped minor/patch updates (ungrouped dependabot on a
   workspace this size is notification spam that gets muted — group or don't bother).
2. Add a scheduled (weekly) workflow job: `cargo audit` + `pnpm audit --prod` —
   report-only at first (no PR gate), so findings surface without blocking work.
3. One paragraph in `docs/contributing/style.md`: how to respond to an advisory
   (assess reachability → bump or pin → note in PR).

**Acceptance.** Dependabot opens its first PRs; the audit job runs green or its
findings are filed as issues; gate stays report-only until the arbiter promotes it.

---

## WS-B — Documentation truth sync (P1)

### B1 — Purge Lead-role remnants repo-wide
**P1 · docs/prompts · Model: Sonnet · Effort: S**

**Problem.** The Lead role was deleted (2-level Orchestrator→Worker is current; the
role enum has two variants; `lead_suffix.md` removed). Live stragglers verified:
- `.claude/skills/prompt-evaluator/SKILL.md:3` and `:17` — references `lead_suffix.md`
  and `.claude/agents/` (also gone).
- `docs/CLI.md` — `--parent — Parent pane ID (orchestrator or lead)` and
  `set-orchestrator … (top-level, can spawn leads/workers)` wording.
- `docs/BUILD.md:72-85` — uses "Lead:" as a machine-role label in the release
  procedure; harmless but collides with the dead agent role. Rename to "Primary:".

**Task.** `grep -rn "lead" docs/ .claude/ src-tauri/prompts/ cli/ --include="*.md"`
(case-insensitive, manually filter false positives like "leads to"); fix every
agent-hierarchy usage. Do NOT touch historical analysis docs
(`docs/gap-analysis-*`, `docs/review-2026-05/`) — they describe the past correctly.

**Acceptance.** The grep returns only historical-doc hits and non-role English usage;
prompt-evaluator SKILL.md describes only files that exist.

---

### B2 — Reconcile `codeterm agent link/unlink/set-orchestrator` docs vs dispatch
**P1 · CLI/docs · Model: Sonnet · Effort: S**

**Problem.** An audit pass claimed these commands are documented in `docs/CLI.md` but
dead. Verification CONTRADICTS this twice over: `cli/src/args/agent.rs:7,17,26`
defines the variants AND `cli/src/cmd/agent.rs:105,115+` dispatches them. The audit
finding was wrong on the static side; only runtime behavior (daemon endpoints they
call) remains unproven.

**Task.**
1. Run each command against the dev daemon (port 7686 only) and record actual
   behavior end-to-end (does the daemon route exist and mutate hierarchy state?).
2. If all work: fix only the stale "lead" wording in `docs/CLI.md` (B1 overlap) and
   close.
3. If a daemon endpoint is dead: decide with the arbiter — wire it or delete the
   command ("no external user base yet" means deleting is cheap now).

**Acceptance.** A short matrix in the report: command → works/dead → action taken;
docs/CLI.md matches reality.

---

### B3 — API.md endpoint sync + a "documented surface" policy
**P1 · docs · Model: GPT-5.5 · Effort: M**

**Problem.** `docs/API.md` (77K) lists endpoints the router no longer registers
(VERIFIED third pass: no `.route` registration for `/api/agents` or
`/api/panes/{id}/children` in `routes.rs`; removal noted in
`docs/HANDOFF-2026-06-06.md:11` but never propagated), and omits dozens of implemented
routes (mem POST/graph-visualization routes, mesh sync/relay/announce variants, web
mirror, debug clock, upload). With ~215 registered routes the doc can't be trusted in
either direction.

**Task.**
1. Generate the ground truth: extract every `.route(...)` registration from
   `src-tauri/src/api/` (routes.rs, web_routes.rs, mesh_routes/) into a list.
2. Diff against API.md. Remove documented-but-gone entries. For
   implemented-but-undocumented: document the agent-facing ones; for internal/web-mirror
   /debug routes add ONE policy paragraph at the top of API.md: "internal surfaces
   (web mirror, debug, test clock) are intentionally undocumented here; see
   `web_routes.rs`" — so the omission is a decision, not drift.
3. Leave a `scripts/` one-liner (or extend the existing `cli-source-of-truth.yml`
   approach) that prints registered-routes-not-in-API.md, so the diff is re-runnable.
   Full automation is B7; this task only needs the script to exist.

**Acceptance.** Re-running the route-diff script reports zero documented-but-missing
routes; undocumented-route count is either 0 or covered by the policy paragraph.

---

### B4 — CLAUDE.md factual sync (evals list, .codeterm/skills list)
**P1 · docs · Model: Sonnet · Effort: S**

**Problem.** Verified drift inside the project's primary agent-instruction file:
- CLAUDE.md lists eval sets `general-agent, mem-recall, mem-protocol, session-restore`.
  Reality: `evals/` contains `cli-agent, dreamer-repair, dreaming, mem-protocol,
  mem-recall, seeds, session-restore, v1_1_23_e2e, _lib`; `evals/README.md` states
  cli-agent REPLACED both the old general-agent AND mem-protocol prompt evals.
  `evals/general-agent/` does not exist while CLAUDE.md (and the prompt-evaluator
  skill) point at it; `evals/mem-protocol/` is a husk (verified: contains only
  `last-report.md`) — delete it or mark it replaced in its own dir.
- CLAUDE.md lists three `.codeterm/skills/` (github-release, prod-build,
  self-install-local). Reality: only `github-release/` exists.

**Task.**
1. Update CLAUDE.md "Behaviour benches" to the real set list (defer to
   `evals/README.md` as source of truth; mark dreamer-repair/seeds/v1_1_23_e2e
   experimental if their READMEs say so — check).
2. For skills: ask the arbiter to choose — (a) create the two missing skill dirs
   wrapping the real logic (`build.sh build` recipe; `scripts/self-install-local.ps1`),
   or (b) trim CLAUDE.md to what exists. Default to (a) ONLY if the wrapped logic
   already exists and is current; otherwise (b).
3. Sync `.claude/skills/prompt-evaluator/SKILL.md` eval-set pointers at the same time.

**Acceptance.** Every path CLAUDE.md names exists; every eval dir is either listed or
explicitly marked experimental in `evals/README.md`.

---

### B5 — `docs/GENERAL_AGENT.md`: rewrite to reality or archive
**P1 · docs · Model: Sonnet · Effort: M**

**Problem.** The doc describes a design that was never built — VERIFIED third pass:
`instructions.rs` contains `test_agent_template_no_markers` explicitly asserting the
template does NOT contain `CODETERM-AUTO-START` (the doc claims those markers preserve
user edits); `EXTRA_INSTRUCTIONS` has zero hits anywhere in `src-tauri/src/`; the
`{connected_services}` placeholder doesn't exist in the GA template (only unrelated
mux-state fields share the name). Plus six unbuilt implementation phases and
Discord/webhook/autostart features. Anyone extending the GA from this doc builds on
fiction.

**Task.** Read `src-tauri/src/service/general_agent/` + `src-tauri/prompts/
general_agent.md`. Rewrite GENERAL_AGENT.md to describe ONLY what is implemented
(template placeholders, render path, file layout, lifecycle). Move the unbuilt design
material to `docs/plans/general-agent-future.md` and index it in PLANS.md — the ideas
are not wrong, they're just not docs.

**Acceptance.** Every claim in the rewritten doc maps to a file:line; the plans doc
holds the rest; both have revision tags.

---

### B6 — WITHDRAWN (audit finding refuted)
**~~P1~~ · docs · third-pass verification**

The audit claimed PROMPTS.md lists `lead_suffix.md` and points contributors to
`.claude/agents/`. Direct grep refutes both: the only `.claude/agents/` mention is the
heading "Role definitions live in `codeterm mem`, not `.claude/agents/`" — i.e. the
doc already says the correct thing — and `lead_suffix` does not appear in the file at
all. **Do not dispatch.** The durable invariant (table rows == prompt files == consts,
3-way) is covered by B7 check 2. Kept here so a future pass doesn't resurrect the
false finding.

---

### B7 — Drift prevention: extend source-of-truth CI to docs
**P2 · CI/design · Model: Opus · Effort: M**

**Problem.** Root cause of B1–B6: docs are synced by hand and nothing fails when they
drift. The repo already solved this once — `.github/workflows/cli-source-of-truth.yml`
validates CLI docs against real commands. Nothing equivalent exists for REST routes,
prompt-file tables, eval-set lists, or revision tags.

**Task.** Design and implement the minimal checks (script + CI step each):
1. Route diff from B3, failing on documented-but-missing routes (missing-but-undocumented
   stays advisory).
2. PROMPTS.md table ↔ `src-tauri/prompts/*.md` ↔ `prompts.rs` consts 3-way check.
3. CLAUDE.md eval list ⊆ `evals/*/` dirs check.
4. Changed `docs/*.md` in a PR without a revision-tag bump → warning (not failure).
5. Config defaults: `docs/CONFIG.md` documented defaults ↔ `src-tauri/src/config/
   schema/` actual defaults diff, plus a dead-option scan (schema fields never read
   outside the schema module). The original Rust audit was asked for this and never
   reported — treat it as unaudited, not clean.
Keep it boring: bash/python scripts in `scripts/`, one workflow `docs-source-of-truth.yml`.

**Acceptance.** Breaking any of the three invariants on a test branch fails the
workflow; docs/contributing gets a short "doc drift gates" section.

---

### B8 — Archive timestamped analysis docs with status headers
**P3 · docs · Model: Codex Spark · Effort: S**

**Problem.** `docs/HANDOFF-2026-06-06.md`, `docs/gap-analysis-*-2026-05-25.md`,
`docs/review-2026-04/`, `docs/review-2026-05/` sit next to living reference docs with
no marker saying "historical snapshot — findings may be fixed". Audit passes (including
this one) keep re-litigating them.

**Task.** Move the two root-level gap-analysis files and the HANDOFF file into
`docs/review-2026-05/` / a new `docs/review-archive/` (arbiter's choice); prepend each
with a 2-line header: `> Historical snapshot (YYYY-MM-DD). Findings may since be fixed;
do not action without re-verifying.` Same header for this folder's docs when closed.

**Acceptance.** `docs/` root contains only living reference docs + dated review dirs;
every review doc has the status header.

---

### A8 — `cli-source-of-truth` workflow has failed on every run, ever
**P1 · CI · Model: Haiku/Sonnet · Effort: S**

**Problem.** Found while landing PR #142 (fifth pass, live CI). The `cli-doctor` job
fails in ~16s on EVERY run — `gh run list --workflow=cli-source-of-truth.yml` shows
8/8 FAIL: `cargo test -p codeterm-cli cmd::manifest` executes at the repo root,
which has no `Cargo.toml` (`error: could not find Cargo.toml`). The CLI-docs drift
gate the testing audit listed as "CI-wired, working" has never executed its tests —
enforcement fiction, same disease as A6. (`chat-card-tests` in the same workflow
passes — the breakage is only the two cargo jobs.)

**Task.** Add `defaults: run: working-directory: cli` to the affected jobs (or per
step `working-directory: cli` — check where `cli/Cargo.toml` actually lives first);
re-run; confirm `cmd::manifest` and `plugin_subcommands_parse` actually execute and
pass. If they fail legitimately once they finally run — report the failures, do not
silence them.

**Acceptance.** Workflow green on a re-run with the cargo jobs visibly executing
tests (non-zero test counts in the log); `gh run list` shows the first-ever pass.

---

### B9 — Translated READMEs: pick a maintenance policy
**P2 · docs · Model: arbiter decision, then Codex Spark · Effort: S**

**Problem.** Nine hand-maintained README translations at root (`README.{de,es,fr,ja,
ko,pt,ru,zh}.md` + en, 12–18K each). Nothing regenerates them; every feature edit to
README.md silently strands nine copies. This is the same drift disease as WS-B but
×9, and no audit pass even sampled them for divergence (assume divergent).

**Task.** Arbiter picks ONE: (a) regenerate all translations from README.md via an
agent pass at each release (add to the github-release skill checklist) — keeps the
polyglot storefront, makes drift bounded by release cadence; (b) prune to en+ru with
a language-banner linking to en; (c) keep all but stamp each with "translated from
README.md revision N — may lag". Then implement. Recommended: (a) — cheap with agents
and matches the project's dogfooding ethos.

**Acceptance.** Chosen policy implemented; the release skill (if (a)) gains the step;
each translation carries the source-revision stamp regardless of choice.

---

### B10 — Shipped `codeterm` skill contradicts the running binary
**P1 · skill/docs · Model: Sonnet · Effort: M**

**Problem.** Found live (fourth pass). The user-level skill at
`~/.claude/skills/codeterm/SKILL.md` (revision 34) — the primary manual every agent
inside CodeTerm loads — disagrees with the prod binary on at least three points:
1. Skill: "exactly 3 roles … Orchestrator → Lead → Worker, enforced by the backend".
   Binary: `spawn --help` lists `Role: Orchestrator, Worker`; repo deleted Lead.
2. Skill documents `codeterm agent status [--pane ID]` — the binary rejects it:
   `unrecognized subcommand 'status'`.
3. Skill's worker-task template tells agents to read `.claude/agents/backend-dev.md`
   — emptied on purpose; roles live in mem.
Every agent spawned in prod gets misinstructed by its own spinal cord — this is the
highest-leverage doc-drift instance in the whole audit.

**Task.**
1. Locate the skill's source of truth (repo root `skills/` dir is the suspect;
   confirm how it syncs to `~/.claude/skills/` / `~/.codeterm/skills/`).
2. Fix the three drifts at the source; sweep the rest of the skill against
   `codeterm --help` output per subcommand (cheap: diff documented commands vs
   `codeterm <cmd> --help` reality).
3. Bump the skill revision; verify the synced copy updates (watcher) or document the
   manual sync step.
4. Feed the "skill ⊆ CLI reality" check into B7 if mechanizable (the CLI already has
   `cli-manifest.json` + `cli-source-of-truth.yml` — extend to the skill file).

**Acceptance.** Skill matches binary on roles, commands, and role-definition
locations; revision bumped; drift check filed or implemented.

---

## WS-C — Prompts & roles (P1/P2)

### C1 — CLI must reject unknown `--role` values
**P1 · CLI · Model: Sonnet · Effort: S**

**Problem.** VERIFIED LIVE against prod (fourth pass): `codeterm agent spawn claude
--role Lead --title LeadProbe …` → exit 0, pane spawned, stored `role=None` — while
`spawn --help` claims "Role: Orchestrator, Worker". Worse than the audit claimed: an
invalid/stale role string is silently swallowed into no-role, which the system treats
as Orchestrator-like ("no role = Orchestrator"), so a mistyped Worker spawn quietly
gains top-level semantics and no parent link (its reports go nowhere).

**Task.** Make role parsing strict end-to-end (CLI arg parse AND the daemon endpoint —
the silent `None` may happen on either side; trace where the string is dropped).
Error message names the two valid roles.

**Acceptance.** `codeterm agent spawn claude --role Lead` (dev 7686) exits non-zero
with a clear message; `--role Worker --parent X` and `--role Orchestrator` spawns
unaffected (re-run the three-provider probe from the fourth pass as the regression
check).

---

### C2 — `worker_suffix.md`: label the report format as suggested, or enforce it
**P2 · prompts · Model: Sonnet · Effort: S**

**Problem.** `src-tauri/prompts/worker_suffix.md` presents a rigid report template
(STATUS/CHANGES/BUILD/FOLLOWUP) that no code parses or enforces — workers can return
anything and the orchestrator gets freeform text. The prompt implies a contract that
doesn't exist.

**Task.** Decide with arbiter: (a) soften wording to "Suggested format:" — zero risk;
or (b) actually parse STATUS into the agent report API so orchestrators can filter
blocked/partial programmatically. If (b), it's a separate designed task — for now do
(a) and file (b) as a plans note.

**Acceptance.** Prompt wording matches enforcement reality. If any prompt text changed:
run the matching eval (`evals/cli-agent/`) before/after — CLAUDE.md mandates benches
for behavioral changes.

---

## WS-D — Testing process (P1) — implements [testing-process.md](testing-process.md)

> The methodology, taxonomy, and snippets live in the companion doc. These tasks are
> the implementation increments, ordered.

### D1 — Kill the prod-port hazard: archive orphaned shell test scripts
**P0 · tests · Model: Sonnet · Effort: S**

**Problem.** Verified twice (second + third pass): `tests/e2e.sh:7` hardcodes
`http://localhost:7685/api` — the PRODUCTION port — plus a Windows-only absolute CLI
path. `final_e2e.sh` likewise
(+ `$APPDATA`). These scripts predate `evals/_lib/lib.sh` (which exists precisely to
prevent prod contamination) and are referenced by nothing in CI or build.sh. Loose at
`tests/` root: `e2e.sh, final_e2e.sh, full-eval.sh, llm-eval.sh, llm-natural-eval.sh,
orchestrator-eval.sh, agent_scenario.sh, test_app.sh, restart-clean.sh, shot.ps1,
parser-test.ts, TESTPLAN.md` (results snapshot from 2026-03-18).

**Task.** For each loose script: grep for references (docs, CI, build.sh, other
scripts). Unreferenced + redundant with pytest-e2e/evals → delete. Anything still
uniquely useful → port onto `evals/_lib/lib.sh` (`dc` wrapper, dev-locked) and move
into `evals/` or `tests/manual/` with a README line. `TESTPLAN.md` → archive per B8
pattern. Nothing that talks to 7685 survives.

**Acceptance.** `grep -rn "7685" tests/` returns nothing executable;
`tests/` root contains only directories + configs; deletions listed in the report.

---

### D2 — Unified test entrypoint: `./build.sh test:all`
**P1 · build · Model: Opus · Effort: M**

**Problem.** Five runners, five invocations, no aggregate: `./build.sh test` (Rust),
`./build.sh e2e` (pytest), `pnpm test:pw` (Playwright), `pnpm test:plugins`
(provider tests), and TS unit tests run via bare `npx tsx` and aren't in any script.
Nobody — human or agent — can answer "is everything green?" with one command.

**Task.** Add `test:all` to build.sh: runs Rust unit → TS unit (add a proper
`test:unit` script using vitest for `src/**/*.test.ts` + `packages/`) → plugin tests →
pytest e2e (only if dev is up — probe `/api/health` on 7686, skip with a loud SKIPPED
otherwise) → Playwright `@smoke` only (same dev-up condition). The old
`tests/webdriver`/WDIO layer has been removed. Print a per-layer summary table and
exit non-zero if any executed layer failed. Layers that need real LLM calls are
excluded from `test:all` by design (see testing-process.md taxonomy).

**Acceptance.** `./build.sh test:all` on a clean checkout with dev down: unit layers
run, integration layers SKIP loudly, exit reflects unit results. With dev up: all
non-LLM layers run. Documented in docs/TESTING.md.

---

### D3 — Playwright tag contract + config enforcement
**P1 · tests · Model: Sonnet · Effort: S**

**Problem.** Tags `@smoke/@golden/@regression/@nightly` exist in specs and are defined
in `tests/playwright/README.md:105-112`, but `playwright.config.ts` (verified third
pass: `workers: 1`, `retries: 0`, `timeout: 90_000`, no `projects` block at all) has
no tag wiring — running "just smoke" relies on the human remembering the grep syntax,
and nothing stops an untagged test from landing. Keep `workers: 1` — the config's own
comment is right that parallel workers would fight over the single live dev console;
projects partition by tag, not by parallelism.

**Task.** Implement the tag contract from testing-process.md §3: named Playwright
projects (`smoke`, `golden`, `full`) with grep filters; npm scripts `test:pw:smoke`
etc.; a tiny lint (script or playwright fixture) failing any `test(...)` whose title
has no recognized tag.

**Acceptance.** `pnpm test:pw:smoke` runs only `@smoke`; adding an untagged test fails
the lint; README and config agree.

---

### D4 — MERGED INTO D3 + D5 (arbiter decision, 2026-06-11)
D3's implementation absorbed the retries config (`golden` project, retries: 1) and
`QUARANTINE.md`; the remaining scope (sweep specs for bare `setTimeout` sleeps,
replace with the shared poll helper) requires D5's fixture layer and is folded into
D5's migration step. Do not dispatch D4 separately.

### ~~D4~~ — Flake policy: retries, quarantine, and LLM-test budgets (original text)
**P1 · tests · Model: Sonnet · Effort: M**

**Problem.** `playwright.config.ts:14` sets `retries: 0` while `@golden` tests make
real LLM calls with fixed sleeps (`cross-pane-chat.spec.ts:44` —
`setTimeout(r, 2_000)` poll; `agents.spec.ts:131-135` — 3s fixed polls, 5-min budget,
zero retries). One slow inference = red suite, and nobody can distinguish "bug" from
"model was slow".

**Task.** Implement testing-process.md §4: `retries: 1` for `golden` project only
(smoke stays 0 — smoke flake is a real bug); replace fixed sleeps with the shared
`pollUntil` helper (D5); create `tests/playwright/QUARANTINE.md` (test, reason, owner,
date, exit criteria) and a `@quarantine` tag excluded from default runs.

**Acceptance.** No bare `setTimeout(...)` sleeps in suites (grep); config diff matches
the policy; QUARANTINE.md exists (empty is fine).

---

### D5 — Consolidate Playwright helpers into fixtures
**P1 · tests · Model: Opus · Effort: M**

**Problem.** Specs share a single `beforeAll` UI connection (one broken test cascades),
roll their own polling loops, and duplicate setup (audit: `smoke.spec.ts:13-15`,
`multiplexer.spec.ts` reset patterns, `mem-core.spec.ts:75` hand-rolled 10×1s retry).
`mesh-stub.ts` runs in-process with a "wedged" mode that can hang the runner with no
stub-level timeout.

**Task.** Build the fixture layer from testing-process.md §2: `test.extend` fixtures
`ui` (per-worker connection with health re-check), `api` (auto-reset after each test),
`pollUntil(fn, {timeout, interval, label})` helper with adaptive backoff and a
diagnostic message on timeout; mesh-stub gets `start({deadline})` and an `afterEach`
guard asserting it was stopped. Migrate 2–3 existing suites as the reference
implementation; the rest migrate opportunistically (file follow-ups, don't big-bang).

**Acceptance.** New fixtures used by ≥3 suites; full suite still passes on Windows dev;
helper has its own unit test (vitest) proving timeout/diagnostic behavior.

---

### D6 — Single source of truth for testing docs
**P1 · docs · Model: GPT-5.5 · Effort: S**

**Problem.** Four documents disagree on layer names and entry points: `docs/TESTING.md`
(3 layers: Unit/E2E/UI), `tests/playwright/README.md` (calls Playwright "e2e"),
`tests/TESTPLAN.md` (adds a CLI-socket layer; results dated 2026-03-18),
`evals/README.md` (benches). New agents get contradictory answers to "which layer do I
write?".

**Task.** Make `docs/TESTING.md` the umbrella: import the taxonomy table from
testing-process.md §1, link down to per-layer READMEs (which keep operational detail
but lose their own taxonomy claims), delete TESTPLAN.md (archived by D1/B8). Add the
"which layer do I write?" decision tree.

**Acceptance.** Exactly one doc defines layers; others link to it; tree present;
revision tags bumped.

---

### D7 — CI test matrix: gate what CAN be gated
**P1 · CI · Model: Sonnet · Effort: M**

**Problem.** CI today runs only `provider-claude` tests (path-filtered to the plugin —
daemon changes don't trigger it), CLI source-of-truth, file sizes, and release builds.
Rust unit tests, TS unit tests, and pytest-e2e are NOT in CI at all. The Playwright
no-CI decision is documented and stands (needs WebView2 + claude auth), but everything
runner-compatible is simply unwired.

**Task.** Add `tests.yml`: job 1 `cargo test --workspace --lib` (ubuntu + windows);
job 2 TS unit (`pnpm test:unit` from D2) + `pnpm test:plugins` triggered on ANY change.
Precision from the third pass: `provider-claude-tests.yml` DOES run on pushes to
develop/master; the gap is PR-time only — its `pull_request` path filter
(`plugins/provider-claude/**` etc.) excludes `src-tauri/**`, so a daemon PR that
breaks the provider contract goes green until after merge. Fix the filter or fold the
job into `tests.yml`. Job 3: optional pytest-e2e against `./build.sh dev --wait` on
the ubuntu runner if headless dev is viable — spike it, don't force it. Document the
matrix (what runs where, what stays local-only and why) in docs/TESTING.md.

**Acceptance.** PR touching `src-tauri/` runs Rust unit + provider tests in CI; the
matrix table in TESTING.md matches workflow files.

---

### D8 — Parity tests must not skip silently
**P1 · tests · Model: Sonnet · Effort: S**

**Problem.** `src-tauri/src/plugin/js_runtime/parity_test.rs` is the only guard keeping
native Rust providers and bundled JS plugins in sync, and it early-returns (skips
silently) when the plugins dir is absent — in exactly the environments (CI) where
nobody notices.

**Task.** Make the skip loud and environment-aware: in CI (env `CI=true`), a missing
plugins dir is a test FAILURE; locally it prints an explicit `SKIPPED: <reason>` via
`eprintln!`. Wire the parity tests into the D7 Rust CI job and confirm plugins are
present there (or build them in the job).

**Acceptance.** CI run shows parity tests executed (not 0-assertions-passed); deleting
the plugins dir locally produces a visible SKIPPED line.

---

### D11 — Migrate hand-rolled TS unit tests to vitest
**P3 · TS/tests · Model: Codex tier · Effort: L · filed from D2's verify-first finding**

All 14 `src/**`/`packages/**` *.test.ts files are hand-rolled tsx scripts with
top-level `process.exit(1)` — incompatible with vitest as-is. D2 shipped a tsx
runner (`scripts/ts-unit-tests.sh`) as the pragmatic `test:unit`; this task is the
real migration: rewrite each file's assert harness into describe/it, swap the
runner script for `vitest run`, keep test semantics identical (count assertions
before/after per file). Sequence AFTER the E-wave stops editing `packages/chat-*`
tests.

---

### D9 — PR template + review checklist: make the evidence rules enforceable
**P1 · process · Model: Sonnet · Effort: S**

**Problem.** testing-process.md §4–§5 require fail-first proof and per-layer evidence
in PRs, but nothing carries those requirements into an actual PR — there is no
`.github/PULL_REQUEST_TEMPLATE.md` and no review checklist anywhere. A process that
lives only in a design doc decays in a week (gap recovered in the second pass — the
first draft of this backlog referenced "the PR checklist" without ever creating it).

**Task.** Add `.github/PULL_REQUEST_TEMPLATE.md` with: layer-justification line for
new tests, fail-first method + pasted evidence (§4), `test:all` output, doc-revision
bumps confirmation, file-size check confirmation. Add the reviewer-side checklist
(sleeps justified? fixtures used? tags present? cleanup paths?) to
`docs/contributing/style.md` or a new `docs/contributing/review-checklist.md`. Keep
the template under ~20 lines — long templates get deleted, not filled. While in
`.github/` (verified: it contains ONLY `workflows/`): add a minimal
`ISSUE_TEMPLATE/config.yml` + one issue form enforcing the `<scope>: description`
title convention CLAUDE.md already mandates.

**Acceptance.** Template renders on a draft PR; checklist linked from TESTING.md;
new-issue flow shows the scope-prefixed form.

---

### D10 — Eval baselines: benches must detect regression, not just produce output
**P2 · evals · Model: Sonnet · Effort: M**

**Problem.** Audit finding (T11): `evals/_lib` snapshots go under `target-check/
evals/` but no eval set stores a last-known-good baseline or fails on regression —
the operator eyeballs a diff and can ship anyway. CLAUDE.md mandates "run the eval
first to capture the baseline" but the capture is manual and unversioned.

**Task.** For each runnable eval set (start with `session-restore` and `mem-recall` —
deterministic-ish): commit a `baseline.json`/`baseline.md` into the eval dir, teach
`run.sh` a `--check` mode that diffs against it and exits non-zero on regression
beyond a declared tolerance; document the update ritual ("intentional behavior change
→ regenerate baseline in the same PR, reviewer sees both diffs"). Model-judged sets
(`cli-agent`) get score-threshold baselines instead of exact diffs.

**Acceptance.** `bash evals/session-restore/run.sh --check` exits non-zero when a
metadata field is deliberately broken (demonstrate once); baseline-update ritual
documented in `evals/README.md`.

---

## WS-E — TypeScript dedup & quality (P1/P2)

### E1 — Finish chat-parsing consolidation Phase 3: delete the TS parsers
**P1 · TS · Model: Sonnet · Effort: S**

**Problem.** Per the in-flight plan `docs/superpowers/plans/
2026-06-10-chat-parsing-dry-consolidation.md` (status: implemented, cleanup pending),
the host (QuickJS `plugin.js`) is now authoritative; the hand-maintained duplicate TS
parsers `plugins/provider-claude/src/parser.ts` (~540 lines) and
`plugins/provider-codex/src/parser.ts` (~300 lines) are unreferenced by the chat flow
(`PaneSessionChat.tsx:469` consumes host-normalized `res.messages`). Two parsers for
one format = guaranteed drift.

**Task.** Execute the plan's own Phase 3: confirm zero import sites of each
`src/parser.ts` (grep `parseJsonlSession` and the module path); confirm `chat_start`
snapshots populate `messages` for claude+codex (run the cross-pane Playwright spec);
delete the parsers and their dedicated tests; keep any test FIXTURES that the plugin.js
tests reuse.

**Acceptance.** `pnpm test:plugins` green; `pnpm test:pw --grep cross-pane` green
against dev; grep finds no references to the deleted modules.

**Risks.** The plan flags the web (non-Tauri) path — verify the web mirror chat also
reads host snapshots before deleting. If it doesn't, STOP and report.

---

### E2 — Extract shared `@codeterm/chat-parsers` helpers
**P2 · TS · Model: Codex Spark · Effort: M**

**Problem.** Claude and Codex providers independently define `extractText`,
`isRecord`, `summarizeInput`, `normalizeToolName`, `stripAnsi`, `idFor` with
near-identical logic (audit F4, file:line pairs in the finding). Every format tweak is
a two-place edit. NOTE: scope this AFTER E1 — if the TS parsers are deleted, the
duplication may now live in the two `plugin.js` files instead; target wherever it
survives (a shared module bundled into both plugin.js builds, or a chat-core export).

**Task.** Locate the surviving duplicates post-E1; extract into
`packages/chat-core/src/parse-helpers.ts` (or a plugin-shared module if plugin.js
can't import workspace packages — check the plugin build first); point both providers
at it; port the duplicated tests to one shared test file.

**Acceptance.** Helper functions defined exactly once (grep); both providers' tests
green; no behavior diff in `recognize.test.ts` / plugin tests.

---

### E3 — Split the two god components: `MessageBubble.tsx` (2216) and `PaneSessionChat.tsx` (1152)
**P1 · TS · Model: Opus · Effort: L**

**Problem.** Verified line counts. `MessageBubble.tsx` mixes markdown rendering,
syntax highlighting, Mermaid + cache, tool icons, clipboard, fullscreen.
`PaneSessionChat.tsx` mixes session lifecycle, optimistic UI, attachments, composer.
Both appear in the failing CI violation list (A5 table) — this task is one of the
splits that lets A5's baselines shrink back out of the allowlist.

**Task.** Two independent sub-tasks, each its own branch:
1. MessageBubble → extract `CodeBlock.tsx`, `MermaidBlock.tsx` (with its cache),
   `MarkdownBody.tsx`, `toolIcons.ts`; MessageBubble becomes composition <400 lines.
2. PaneSessionChat → extract `useChatSession` hook (lifecycle + event subscription),
   `ChatComposer.tsx`, `useAttachments` hook.
Pure mechanical extraction — no behavior or styling changes; prop drilling is fine for
round one, don't introduce contexts "while we're here".

**Acceptance.** Both files <1000 (target <500); `pnpm test:plugins` +
`pnpm test:pw:smoke` green; visual spot-check of a chat pane with code block + mermaid
+ tool cards (screenshot via `/api/screenshot`); file-size check green.

---

### E4 — Validate untrusted JSON at the boundaries
**P2 · TS · Model: Sonnet · Effort: M**

**Problem.** Chat/plugin paths `JSON.parse` external input and proceed on shape
assumptions via `as Record<string, unknown>` casts (audit F9/F12: provider parsers,
`src/lib/webApi.ts` responses). Malformed entries render broken cards silently.

**Task.** Add lightweight runtime guards at the boundary only (don't Zod the whole
app): a `parseEntry(line): Entry | null` per provider that validates the 3–4 fields
actually consumed and returns null (logged once per session) on mismatch. Post-E1 this
means the QuickJS plugin.js parsers — keep the guards dependency-free (QuickJS has no
npm). webApi.ts: guard only endpoints whose responses feed rendering.

**Acceptance.** Feeding a corrupted JSONL fixture produces a skipped-entry log, not a
broken card (add this as a plugin test case); no new runtime deps in plugin.js.

---

### E5 — Delete dead plugin bridge code
**P3 · TS · Model: Codex Spark · Effort: S**

**Problem.** `src/lib/plugins/messageEvents.ts:36-50` — `messagesToEvents()` and
`makeLineParser()` have zero call sites (audit F6; independently re-verified in the
second pass — grep across src/, plugins/, packages/ finds no references outside the
defining file); they bridge an old `parseLine` contract no plugin uses.

**Task.** Re-verify zero call sites (grep including plugins/ and tests); delete both
functions plus now-unused types/tests; if `parseLine` remains in the plugin type
surface (`src/lib/plugins/types.ts`) with no implementor, flag that to the arbiter as
a follow-up question rather than deleting the type unilaterally.

**Acceptance.** `pnpm test:plugins` + TS build green; grep clean.

---

### E6 — Detection fingerprints: define "structural" and harden the literals
**P2 · TS/design · Model: Opus · Effort: M**

**Problem.** CLAUDE.md mandates "structural fingerprints, never literal strings" for
agent detection, but detection code matches exact vendor copy:
`plugins/provider-claude/src/detect.ts:38-43` (`"esc to interrupt"`,
`"? for shortcuts"`), `plugin.js:13-23` literal array, codex `"codex/"` substring. One
vendor copy change breaks detection — the exact failure mode the rule exists for.

**Task.**
1. First write the missing definition: add to `docs/contributing/architecture.md` what
   counts as structural (multi-signal, layout/glyph based, regex with alternation over
   historical variants, confidence scoring) vs literal (single exact copy string).
2. Then refactor the detectors to it: each provider keeps a scored signal list (glyphs
   like `✱✻`, footer shape, prompt-char patterns, title patterns) where no single
   vendor-copy string is load-bearing; keep current literals as ONE signal among
   several, never the sole gate.
3. Add detection tests with historical UI variants (collect from existing fixtures).

**Acceptance.** Removing any single literal string from a fingerprint set still detects
current fixtures; architecture.md defines the rule concretely; detection tests green.

---

## WS-F — Rust security & structure (P0/P2)

> Security context: `docs/SECURITY-AUDIT-2026-06.md` already tracks a separate
> remediation list (spawn-arg quoting, plugin host confinement — partially landed on
> this branch). Executors check it before starting any WS-F security task to avoid
> duplicating or colliding with in-flight remediation.

### F1 — Token validation: hardcoded TTL + non-constant-time compare in web_routes
**P0 · security · Model: Sonnet · Effort: S**

**Problem.** VERIFIED in the second pass (working tree, branch `security`) — three
sites in `src-tauri/src/api/web_routes.rs`:
- `:161` — `api_token == candidate`: plain equality on token material, while
  `auth.rs` deliberately uses `services::tokens::constant_time_eq` for the identical
  check. Timing side-channel on the manifest endpoint.
- `:168` — `Duration::from_secs(3600)` hardcoded where `auth.rs:86` uses
  `crate::api::routes::WEB_TOKEN_TTL` — changing the constant silently desyncs the
  manifest endpoint's token lifetime.
- `:193` — cookie `Max-Age=3600` hardcoded in the Set-Cookie string: third copy of
  the same TTL. Cookie lifetime must derive from `WEB_TOKEN_TTL` too, or an extended
  TTL produces cookies that die before the token does (and vice versa).

**Task.** Replace `==` with `constant_time_eq`; derive both `3600`s from
`WEB_TOKEN_TTL` (cookie via `WEB_TOKEN_TTL.as_secs()`); add a unit test asserting the
two validation paths accept/reject the same token at TTL±ε (inject a clock or compare
the constants directly). Note this branch is `security` — coordinate with uncommitted
changes in these exact files (`web_routes.rs`, `auth.rs` are dirty in the working
tree).

**Acceptance.** Grep finds no `3600` token literals in api/; no `==` on token material
in web_routes.rs; `cargo test api::` green.

---

### F2 — Split `mesh/mod.rs` (1276 lines; mod.rs target <600)
**P2 · Rust · Model: Opus · Effort: L**

**Problem.** Verified 1276 lines — 2× the mod.rs facade target; the advisory list also
shows `dispatch/mod.rs` 826, `cli/src/cmd/mem/mod.rs` 796, `config/schema/mod.rs` 773,
`bridge/manager/mod.rs` 699, `plugin/mod.rs` 644.

**Task.** Start with mesh/mod.rs only (largest, highest churn area): extract peer
discovery, event handling, and sync logic into sibling modules; mod.rs keeps types +
facade. The other five become follow-up entries here once mesh proves the pattern.
Mechanical move; preserve lock ordering (mux first); grep nested `.lock()` after.

**Acceptance.** `mesh/mod.rs` <600; `cargo test mesh::` green (PowerShell foreground,
narrow filter); `./build.sh dev` boots and mesh tab functions (manual smoke).

---

### F3 — Provider boilerplate: document the canonical layout
**P3 · Rust/docs · Model: GPT-5.5 · Effort: S**

**Problem.** Four provider impls follow the same submodule pattern
(detection/sessions/launch/schemas + trait delegation) at 460–1400 lines each (audit
R8). This is structural, not copy-paste — a macro would be premature — but the pattern
is undocumented, so the next provider will be shaped by whichever file its author
happens to read.

**Task.** Write the provider template into `docs/contributing/architecture.md`:
required submodules, size budgets, what belongs in Rust vs the JS plugin variant, and
that `parity_test.rs` is the sync contract. No code changes.

**Acceptance.** Section exists with a file-tree sketch; reviewed against all four
current providers for accuracy.

---

### F4 — `service/` vs `services/`: one of these names is lying
**P2 · Rust · Model: Sonnet · Effort: S**

**Problem.** Verified: both `src-tauri/src/service/` (the real subsystem home —
orchestrator, pane, pty_io, spawn, hierarchy, general_agent, pane_chat, ~20 files)
and `src-tauri/src/services/` (four small files: `activity.rs`, `push.rs`,
`smart_add.rs`, `tokens.rs`, a 104-byte mod.rs) coexist. The original Rust audit was
explicitly asked to explain this and silently dropped it. Looks like an unfinished
extraction with no charter — every future contributor (and agent) flips a coin on
where new service code goes. Note `services::tokens::constant_time_eq` is
security-load-bearing (used by `api/auth.rs`, and F1 adds a caller).

**Task.** Ask the arbiter for the intended split, then enforce it. Default
recommendation: fold the four `services/` files into `service/` (or a better-named
home: `tokens.rs` arguably belongs near `secrets/`) and delete `services/`. Pure
`mod`-path mechanical move; fix all `use` paths; sequence AFTER F1 lands to avoid
churning the same imports twice.

**Acceptance.** Exactly one of the two directories remains; `./build.sh check` green;
a one-line placement rule added to `docs/contributing/style.md` ("daemon subsystems
live in `service/`").

---

### F5 — Error-handling sweep: unwrap/expect/swallowed errors in daemon paths
**P2 · Rust · Model: Sonnet (inventory) · Effort: M**

**Problem.** The original audit was asked to sweep `unwrap()`/`expect()` in non-test
code (api/, service/, multiplexer/), inconsistent error types, and silently swallowed
errors (`let _ =`, `.ok()`); it reported on route-error style only and dropped the
sweep. Status: UNAUDITED — a long-running daemon hosting user PTYs and agents must
not panic on a malformed request or a poisoned cache entry.

**Task.**
1. Inventory: `grep -rn "\.unwrap()\|\.expect(" src-tauri/src/{api,service,services,
   multiplexer,mesh}/ --include="*.rs"` excluding `tests`; classify each hit:
   (a) provably-infallible (lock on non-poisoning parking_lot, const parse) — leave,
   (b) request-path panic risk — fix to error return, (c) startup-path — `expect`
   with a message is acceptable, ensure the message says what broke.
2. Same pass for `let _ =` on fallible daemon operations — each either logs or gets a
   comment naming why dropping is correct.
3. Fix category (b) only; report counts for the rest. No blanket refactor, no new
   error-handling framework — the `(StatusCode, String)` route style stays (audit R9
   accepted it).

**Acceptance.** Report with counts per category; zero category-(b) hits remain in
api/ request handlers; `cargo test` green; no new dependencies.

---

### F6 — Split `cli/src/cmd/plugin.rs` (1743 lines)
**P2 · Rust · Model: Opus · Effort: M**

**Problem.** Found only in the third pass (every prior audit missed it; an output
truncation hid it from the second): the single largest CLI source file at 1743 lines —
74% over the hard limit, bigger than anything in the daemon.

**Task.** Map subcommand groups; split into `cmd/plugin/` module by group (read the
file first — don't trust guessed seam names), `mod.rs` as dispatch facade <600.
Mechanical; CLI output must stay byte-identical (snapshot a few command outputs
before/after).

**Acceptance.** All resulting files <1000 (facade <600); file-size check green for
this path; CLI test invocation green; before/after output snapshots match.

---

### F7 — `workspace create --cwd` bug: ignores name, returns existing ws, leaks a tab
**P1 · Rust daemon · Model: Sonnet · Effort: S**

**Problem.** Found live on prod (fourth pass), reproduced twice:
`codeterm workspace create "spawn-test" --cwd C:/Developer/codeterm` — when a
workspace with that cwd already exists — does NOT create a workspace, silently
ignores the requested name, returns the EXISTING workspace's info (exit 0), and
**creates a stray empty tab in it on every call** (panes `33401735` "Bhabha",
`509a868b` "Cantor" appeared in the `codeterm` workspace). Also: `workspace create
"X"` without `--cwd` yields `cwd: null` and `tab_count: 1` with `pane_ids: []`
(inconsistent counters). The cwd-dedupe may be intentional UX, but silent name
discard + tab leak is not.

**Task.** Find the create handler (daemon side, likely `commands/` or `api/routes`);
decide with the arbiter: dedupe-by-cwd stays but returns an explicit
`"existing": true` marker, never leaks a tab, and the CLI prints "workspace with
this cwd already exists: <name>"; or dedupe goes away entirely. Fix the
null-cwd/tab_count inconsistency. Add a pytest-e2e case for both shapes.

**Acceptance.** Repro sequence from this finding produces either a new workspace or
an explicit already-exists message; zero stray tabs after 3 repeated calls; e2e test
covers it.

---

## WS-G — Multi-agent onboarding (P1/P2)

> Design rationale lives in [agent-team.md](agent-team.md) §1; these are the
> dispatchable mirrors.

### G1 — Generated `AGENTS.md` + generator script + CI staleness check
**P1 · tooling · Model: Opus (section translation) + Sonnet (wiring) · Effort: M**

**Problem.** OpenAI Codex agents opening this repo see no instructions at all — no
`AGENTS.md` (Codex's native instruction file). CLAUDE.md is canonical and
Claude-specific. The routing rubric assigns real tasks to GPT-5.5/Codex Spark
workers; today they'd execute without the hard rules (prod 7685, no auto-commit,
lock ordering).

**Task.** Per agent-team.md §1 decision: `scripts/gen-agents-md.sh` (CLAUDE.md →
AGENTS.md with the generated-file header and Claude-harness sections translated to
vendor-neutral wording); staleness check appended to B7's workflow; no other vendor
formats. The generator header must state that the "never edit AGENTS.md —
auto-generated by CodeTerm" user-global rule refers to runtime GA home-dir files, not
this repo artifact.

**Acceptance.** AGENTS.md exists, generated; editing CLAUDE.md without regenerating
fails the CI check on a test branch; a Codex session in the repo demonstrably picks
up the rules (paste evidence).

### G2 — Sync `docs/contributing/agents.md` with current reality
**P2 · docs · Model: Sonnet · Effort: S**

2-level hierarchy only, mem-based role skills, AGENTS.md story, link to the
model-routing rubric. Remove `.claude/agents/*.md`-era instructions.
**Acceptance.** No stale role/path references; revision bumped.

### G3 — RESOLVED (verified live, fourth pass)
**~~P1~~ · tooling · closed 2026-06-11**

`codeterm agent spawn` supports `--model` as an opaque pass-through, with discovery
via `codeterm agent providers` / `codeterm agent models <provider>`. Live probe
against prod: `spawn claude --model haiku`, `spawn codex` (booted gpt-5.5 medium),
and `spawn opencode` all spawned, executed a task, replied via `send-text`, and
delivered `agent report` to the parent. Available: codex = gpt-5.5 (default) / 5.4 /
5.4-mini / 5.3-codex / 5.2; claude = opus 4.7 / sonnet 4.6 / haiku 4.5; opencode = 7
models. The third-pass doubt (CLI.md:41 "interactive picker") applies only to
mid-session model SWITCHING, not spawn-time selection. Note: "Codex Spark" from the
rubric is the Spark pane's runtime; map the rubric tier to a concrete codex model id
when first dispatching to it (check with `codeterm agent list --json`).

---

### G4 — GA startup auto-acknowledge (filed from live failure, 2026-06-11)
**P1 · daemon · dispatched to Gpt5.5-new** — the dev GA Main pane sat dead on
claude's interactive "config invalid: Exit/Reset" menu. GA bootstrap must detect
known blocking provider menus (structural patterns) and auto-answer with the
recoverable option + emit a warning notification. In flight.

### LAZY-1 — Eager PTY spawn for headless/API pane creation (user-reported, verified)
**P1 · daemon+tests · dispatched to Gpt5.5.** Panes created via API have no PTY
until a UI mounts them (lazy-load optimization: inactive tabs unrendered;
TerminalPane spawns PTY on mount). Headless tests of background panes are
therefore fragile (compensated today by activation + long waits, e.g. the 15s
`fresh_pane`). Fix: `eager: bool` on create surfaces (REST/socket/CLI), daemon-side
spawn, mount-time attach guard, e2e helpers switch to eager, new non-active-pane
e2e test. In flight.

### T-RAT — E2E suite rationalization (user-reported: "куча пустых терминалов")
**P1 · tests · QUEUED (blocked until E2E-auth + LAZY-1 land — all three edit
conftest.py).** The pytest harness opens a fresh workspace+terminal per test even
for API-shape tests that never touch the PTY; with lazy-PTY waits this is the
dominant suite cost and litters the dev UI. Scope: (1) audit every tests/e2e test:
classify needs-PTY / API-only; (2) API-only tests get a no-pane fixture (pure REST
client); (3) needs-PTY tests share a module-scoped eager pane where isolation
allows, fresh eager pane only where state is mutated; (4) measure: suite wall-time
before/after (paste both); target — no terminal exists that no test types into;
(5) VACUITY SWEEP (arbiter code-review finding): every output assertion that can
match the typed command's terminal echo is vacuous — `test_exec_echo` even admits
it ("at least the command echo"). Convention: split markers
(`echo 'EAGER_''OK'`) so the asserted string exists only in real command output;
sweep every wait_for_output/contains assertion in tests/e2e and fix. This
convention also goes into the future docs/contributing/writing-tests.md.

### R1 — Research: concurrent claude sessions corrupt `~/.claude.json`
**P1 · research · NOT dispatched.** Evidence: live corruption during swarm
operation — a fully valid JSON document followed by 865 bytes of a stale version's
tail (non-atomic write without truncate) + 12 garbage bytes inside string values;
claude itself had already quarantined a `.corrupt-*` copy earlier, so this is
recurring. Salvaged by truncating to the first valid document. Root cause is in
the claude CLI (upstream), but CodeTerm amplifies it by spawning many concurrent
claude sessions. Mitigations to research: stagger spawns, watchdog that validates
the file and auto-salvages (the raw_decode-truncate recipe works), or upstream
issue. Until then: the salvage recipe is in the session log.

## Post-merge backlog (after PR #143 lands on develop)

Tracked here so nothing silently drops; dispatch after the merge, briefs on demand.

| ID | Scope | Notes |
|----|-------|-------|
| WEB-COV | Web gate functional coverage (user-requested 2026-06-12): the token-link browser UI incl. phone/mesh usage. Fast tier: non-llm e2e on `web_routes.rs` auth boundaries (no/bad token → 401, good token → SPA + API access). Slow tier: Playwright parametrized to the web entry (same React bundle over HTTP + token) — core cross-pane chat smoke in a mobile viewport, mesh send from the web client, PwaSetup flow. F1 covered token *security*; this covers *functionality*. | new |
| CLI-AUDIT | Read-only inventory of the `codeterm` CLI surface: duplicate/overlapping commands, dead legacy, flag inconsistencies, undocumented or untested commands; kill-list proposal (no external users — delete aggressively). Brief: tasks/CLI-AUDIT.md. | dispatched (research may run pre-merge, read-only) |
| MODELS-PROBE | Nightly llm-tier probe: every provider × advertised model boots, answers one prompt, sid persists. | held |
| MESH-COV / MEM-COV | Deepen mesh + mem e2e beyond the current core smokes (relay edges, graph-repair surfaces). | held |
| TTL-CLOCK | TTL watchdog xfail → real test on injectable debug clock. | held |
| D11 | Vitest migration of remaining legacy TS tests. | held |
| CI-ON-EIGHT | Mesh-CI: macOS native + Linux + Windows-ARM64 VM rig on Eight (M5; live WIP in its "Testing plan" pane — QEMU Win11 unattended install running 2026-06-12), plus BLACKSTAR native Windows; triggered via codeterm over mesh, posting gh commit statuses. Three integration points: (1) PR gate — push → 3-platform run → commit status; (2) pre-release gate in the github-release skill before tag/build; (3) on-demand cross-platform repro (e.g. the spawn+send daemon crash). Restores the coverage removed from hosted CI. | program |
| LLM-RERUN | Re-run the LLM e2e tranche when provider limits reset. | held |
| SUITE-SPLIT | Split the hierarchy mega-suite; biome/clippy ratchet drains; file-size allowlist drain to zero. | held |
| R1 | (see WS-G above) claude.json concurrent-corruption research. | held |
| RESUME-SKIP-SIGNAL | Stale/never-materialized sid → restore silently starts a fresh session, no UI hint or notification; user caught it only by eyeballing (2026-06-12, rm-claude-1). Add a pane badge + notification on resume-skip. | new |
| SEND-VERIFY-GAP | Verified-send returns ok yet the provider never records the input. Two live instances on 2026-06-12: matrix claude pane (send ok, JSONL never materialized) and a prod worker spawn whose --task left only mojibake in the codex composer. Strengthen agent-pane send verification against provider-store materialization (the matrix pre-kill gate is the prototype) or document the weaker guarantee honestly. | new |

## Dispatch order (arbiter cheat-sheet)

Live roster (workspace `codeterm`, verified): **Lamarr** `5c64a651` = arbiter
(Fable, Orchestrator) · **Opus** `232c0b4a` · **Sonnet** `228012ad` · **Haiku**
`35a191bc` · **Spark** `8b153ef9` (Codex) · **Gpt5.5** `a10ccdbd`. Ephemeral extra
workers are spawned per task (`spawn … --role Worker --parent 5c64a651 --model …`,
verified working). Standing panes get tasks via `send-text`; their return path is
`codeterm send-text "…" --pane 5c64a651` (they are not hierarchy children, so
`agent report` from them does not route to the arbiter).

| Wave | Tasks | Executor (pane) |
|------|-------|-----------------|
| 0 (now) | W0 — finish & land `security` (pre-executes A5 baselining) | Opus — sole writer on the shared tree |
| 1 | A1, F1, D1, A2 | A1 → Opus; F1 → Sonnet; D1, A2 → Haiku/Sonnet |
| 2 | A6, B1, B2, B4, C1, F7, B10, G1 | C1+F7 → Sonnet; B1/B2/B4 → Haiku/Sonnet; B10 → Sonnet; G1 → Opus design + Sonnet wiring |
| 3 | D2, D3, D4, D5, D6, D7, D8, D9 | D2/D5 → Opus; D6 → Gpt5.5; rest → Sonnet |
| 4 | E1, E3, B3, B5, A4, A7, F4 | E3 → Opus; B3 → Gpt5.5; A4 → Spark+Sonnet; rest → Sonnet |
| 5 | B7, B8, B9, C2, D10, E2, E4, E5, E6, F2, F3, F5, F6, A3, G2 | per task headers |

40 dispatchable tasks (B6 withdrawn — refuted; G3 resolved — closed). Tier totals:
Sonnet ×27 · Opus ×10 · GPT-5.5 ×3 · Codex/Spark ×5 · Haiku — cheap verification and
S-effort doc tasks throughout. GPT-5.5 tasks remain gated on G1 (AGENTS.md) so Codex
workers get the hard rules. See [agent-team.md](agent-team.md) for the rubric.

<!-- revision: 7 -->
