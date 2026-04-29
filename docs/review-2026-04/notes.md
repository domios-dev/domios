# Docs review 2026-04-29

End-to-end documentation review of the codeterm repo against HEAD on
`origin/develop` after the v1.1.0 release. Focus: stale CLI examples,
dead REST endpoints, dead file links, and translation drift.

This file is the deliverable for the review pass; delete on merge.

## Fixed (15 items)

- README.md:204 — replaced non-existent `POST /api/panes/{id}/split`
  example with `POST /api/agents/spawn` (only valid way to create a
  child pane today).
- README.md:221 — removed non-existent `codeterm pane split PANE_ID`
  CLI example. The `pane` subcommand has no `split` action.
- README.ru.md:187,204 — same fix as README.md:204/221, in Russian.
- README.zh.md:186,203 — same fix, in Simplified Chinese.
- README.ja.md:186,203 — same fix, in Japanese.
- README.de.md:186,203 — same fix, in German.
- README.es.md:186,203 — same fix, in Spanish.
- README.fr.md:186,203 — same fix, in French.
- README.pt.md:186,203 — same fix, in Portuguese.
- README.ko.md:186,203 — same fix, in Korean.
- docs/API.md:247-249 — removed three documented `/api/setup/skills*`
  endpoints that have no route handler (`/api/setup/skills`,
  `/api/setup/skills/{id}/install`,
  `/api/setup/skills/{id}/uninstall`). Section heading updated from
  "Setup (Skills, Rules, Hooks)" → "Setup (Rules, Hooks)". Bumped
  revision 31 → 32.
- docs/API.md:296 — removed documented `GET /api/mem/recall` endpoint.
  No such route exists in `src-tauri/src/api/routes.rs`; the only mem
  search surface is `GET /api/mem/search`. (`recall_with_config` is an
  internal method called by `mem_search`, not its own endpoint.)
- docs/PLANS.md:7 — removed dead link to deleted
  `plans/architecture-evolution.md` (deleted in 2f2e2de stale-plans
  cleanup but the index still pointed at it).
- docs/PLUGINS.md:195,232 — replaced two dead references to
  `plans/architecture-evolution.md` with pointers to existing plans
  (plugin-architecture.md) since that's where the relevant rollout
  notes now live.
- docs/MEMORY.md:311 — removed dead reference to deleted
  `docs/plans/mem-architecture.md`.
- docs/plans/mem-visuals.md:6 — removed dead `Parent:` reference to
  deleted `docs/plans/mem-architecture.md` (mem-visuals is in-flight,
  not shipped, so the dangling parent pointer is a real defect).
- .claude/agents/backend-dev.md:34 — replaced
  `docs/plans/architecture-evolution.md` reference with
  `docs/ARCHITECTURE.md` and `docs/plans/plugin-architecture.md`.
- .claude/agents/code-reviewer.md:95 — same dead-link fix.
- CHANGELOG.md (v1.1.0 narrative) — fixed dates: pty cleanup fix
  merged 2026-04-26 (commit 8227b5e), wake-up auto-injection merged
  2026-04-29 (Merge PR #3, commit 4eb6088). Original copy bundled
  both as "2026-04-28" which matched neither commit nor merge dates.

## Found, NOT fixed (need human decision) (4 items)

- README.md:121 — Build instructions tell users to clone with
  `git clone https://github.com/rollacode/codeterm.git`, but
  `rollacode/codeterm` is currently a **private** GitHub repo
  (verified via `gh repo view`). Anonymous clone returns 404. Same
  for `https://github.com/rollacode/codeterm/releases` (used in
  Releases links across all 9 READMEs). Need a human decision: either
  flip the repo to public, or document an alternative install path.
  Flagged here because the README is presented as if the project is
  public.
- README.md:106-110 — Windows install via `scoop bucket add codeterm
  https://github.com/rollacode/scoop-codeterm`. The `scoop-codeterm`
  repo also exists but is private — same 404 for anonymous users.
  Will fail for any user who tries it. Same call: publish or rewrite.
- src-tauri/src/mem/mod.rs:1, src-tauri/src/mem/mod.rs:22,
  src-tauri/src/mem/dreaming.rs:7, src-tauri/src/service/sync_watcher.rs:4
  — four Rust source files reference deleted
  `docs/plans/mem-architecture.md` in module/file doc comments.
  Out of scope for this review (rule: don't fix code from a docs
  review). Worth a separate code-cleanup pass — backend-dev should
  delete these dangling references or repoint to whatever now plays
  the historical-design-doc role for mem.
- README.md:148-156 — Build section tells users to run `pnpm tauri
  dev` and `pnpm tauri build`. CLAUDE.md and `build.sh` are the
  canonical entry points used by maintainers (`./build.sh dev`,
  `./build.sh build`). README and BUILD.md disagree on which is the
  recommended command for new contributors. Not factually wrong (both
  paths work), but inconsistent. Owner choice.

## TRANSLATION-TODO (added to .ru.md, .zh.md, .ja.md, .de.md, .es.md, .fr.md, .pt.md, .ko.md — 8 markers)

Inserted a single HTML-comment block at the top of each translated
README listing the same three drifts:

1. Top-of-features GIF subsections (`### Agent Orchestration`,
   `### Cron Jobs`) present in `README.md` lines 32-39 are missing
   from every translation.
2. The English README has 4 per-OS install subsections (`### macOS
   (Homebrew)`, `### Windows (Scoop)`, `### Windows (installer)`,
   `### Linux`); every translation collapses these into a single
   "Download" section. Either match English structure or accept the
   delta — flagging for the human translator.
3. The English section name `### Canvas Panels` is rendered in every
   translation as "Built-in Browser" / "Integrated Browser" /
   equivalent. The feature was renamed; translations did not follow.

No fresh translation written — that's a human translator's call. The
8 TRANSLATION-TODO comments will be visible to anyone editing the
files until they're addressed and removed.

## Other notes

- CHANGELOG.md commit-hash references all check out (verified every
  cited SHA exists in git log v1.0.1..origin/master). The narrative
  count "Seven feature branches plus three cleanup followups" is
  approximate — actual merge count on 2026-04-25 was 8 feature
  branches plus 4 followup commits — but this is decoration, not a
  factual claim, so left as-is.
- URL liveness check: 39 distinct external URLs across the docs.
  Three GitHub URLs return 404 anonymously, all because the repos
  are private (rollacode/codeterm, rollacode/codeterm/releases,
  rollacode/scoop-codeterm) — see "Found, NOT fixed" above.
  `docs.a gateway.ai` is unreachable from the reviewer's network due
  to a Latvian-ISP DNS sinkhole; assumed alive. Everything else
  returned 200/301/302.
- Auto-generated files left untouched: CLAUDE.md, AGENT.md,
  AGENTS.md (per CLAUDE.md project rule).
