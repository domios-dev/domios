# plugins-rethink — Final Polish + Iterative Review Loop

> Owner: Dante (orchestrator). The "last things" after the hardening campaign +
> the iterative Codex review LOOP Andrey asked for (which the first campaign did as
> a single breadth round, not the converge-until-dry loop). Branch `plugins-rethink`
> @ 8d7fa160. Release stays PARKED.

## Track 1 — Iterative Codex review loop (the centerpiece)
**Loop:** each round = Codex re-reviews → Dante triages valid → fix workers → re-review.
Repeat **until a round surfaces no new valid HIGH/MED** (loop-until-dry), cap ~5 rounds.
Round 1 = the campaign's breadth pass (4 slices) — already done. This is round 2+:
- **Round 2 focus:** adversarially re-review the wave-1/2/3 FIXED code — did the fixes
  introduce bugs or miss edges? (reopen peek-then-delete; hot_reload lock restructure;
  async spawn_blocking closures; detection refuse-ambiguous; bitwarden async unlock;
  provider-claude split; CRLF normalize). Plus a fresh diff-vs-master sweep for anything
  round 1's 4 slices under-covered.
- 2 reviewers, different angles (R-A = correctness/races/edges on the fixes; R-B =
  fresh full-diff sweep). READ-ONLY; emit `SEVERITY | cat | file:line | what | fix`.
- Triage → fix valid → **round 3** re-review of the new fixes → … until dry.

## Track 2 — deferred / optional items (parallel where actionable)
- **T2.1** web-access PW **test-testid typo**: spec queries `web-ui-serve-hint`; element is
  `web-access-serve-hint` (NetworkTab.tsx:269). One-line test fix. (Verify via ROG.)
- **T2.2** keybinding-display: `renderKbd` shows the raw config value with no Mod→Ctrl
  resolution for display (KeybindGroup.tsx:55). Investigate → if a real per-platform
  display bug, resolve `Mod` for display; else fix the test expectation.
- **T2.3** FE claude-hardcoded spawn defaults → capability/registry-driven: `spawnClaudeTab`
  (index.tsx:387), CommandPalette "Spawn Claude" (:510), MemoryPanel AGENTS=[claude],
  `host_compute.rs` default backend. LOW.
- **T2.4** daemon-shells-its-own-CLI in-process refactor: `cli.rs run_plugin_cli` +
  web_routes plugin ops fork the codeterm CLI. Move plugin-management logic into an
  in-process module shared by daemon + CLI; routes/commands call it directly. (Bigger;
  runtime-freeze already mitigated by spawn_blocking — this is the architectural cleanup.)
- **T2.5** extensions addChannel closes the WebView2 page (PW): no reload in app code →
  harness-only by elimination. **Needs a human GUI repro** (Andrey hand-adds a channel in
  dev; ROG watches health/logs) to fully settle app-vs-harness. Blocked on Andrey.
- **T2.6** lmstudio on ROG dev — install FROM the codeterm-plugins repo (now on origin 0e4ec31, no
  file-push needed): ROG clones/pulls codeterm-plugins, `plugin install --from-dir <repo>/lmstudio` into
  ~/.codeterm-dev, sets baseUrl=http://192.168.1.126:1234 (Andrey's LM Studio server), refresh dev,
  re-run 17-open-shell -> the 5 should run live on Windows.

## Dispatch (this round)
1. Review-loop Round 2 — 2 Codex reviewers (R-A fixes-focus, R-B full-diff sweep).
2. FE-polish worker — T2.1 + T2.2 + T2.3 (all FE/TS; local tsc+test:unit, ROG verifies T2.1).
3. daemon-CLI worker — T2.4 (Opus, careful refactor + gates).
4. ROG ask — T2.6 install lmstudio on dev.
5. T2.5 — flag for Andrey (manual repro).

Gate every fix with the pre-push hook; merge → re-review (loop). Stop when review is dry.

<!-- revision: 1 -->
