# Self-healing detector loop — close the bench↔live gap automatically

> From Andrey 2026-06-21, after live testing kept finding picker/state shapes the green bench
> missed. The root problem: "bench green" did NOT predict "works live." This plan names the gaps
> and builds a SELF-ITERATING loop (harvest live → judge → add case → fix → converge) that hardens
> the detector against real agent variety without hand-picking shapes or shipping patches/legacy.
> Branch `plugins-rethink`.

## Why every live test failed — the 5 gaps
1. **Deployment gap (biggest).** The new attributed-grid detector is BENCH-ONLY. The live UI runs
   the OLD per-plugin text parser. So every live test exercised code we already replaced — e.g.
   cursor multi-step wizard ALREADY parses perfectly in the bench but fails live. → fixed by Phase 2
   (wire detector into prod, in flight).
2. **Coverage gap.** The corpus is ~5 hand-picked shapes; real agents emit dozens (side-preview
   wizards, key-shortcut trust prompts, prose questionnaires, per-provider multi-step variants…).
   Green-on-5 says nothing about the long tail. → the harvest loop below.
3. **Verification gap.** The "final test" = `testbench/run.sh` green on that narrow corpus. It is
   NOT a live oracle — it can't tell us a real card renders correctly. → an LLM-judge oracle that
   compares the RAW screen to the detector's output.
4. **Heuristic gap.** Per-provider/glyph rules are brittle; the attributed-grid + structural-signal
   rule (a card only on cursor-hidden/alt-screen OR selection-attribute OR picker-footer; prose →
   none) is the right basis but must be hardened across real variety by the loop.
5. **Worker-quality gap.** codex (gpt-5.5) workers are fine for mechanical capture but weaker on the
   harder detector reasoning + judging. → use OPUS subagents for the judge and the fixer.

## The self-healing loop (harvest → judge → corpus → fix → converge)
One iteration, repeated until convergence (≈20-30 reps max, stop after K consecutive clean rounds):

1. **Harvest (diverse).** Drive a real agent via dc (rotate provider ∈ {claude,codex,cursor,
   opencode} × a PROMPT-BANK that elicits varied surfaces: model picker, A/B/C choice, multi-step
   wizard, multi-select, permission, trust/auth, prose-questionnaire-negative, plain work). Capture
   the active-frame `raw.cast` + rendered screen, and run the CURRENT native `detect_picker`/
   classifier on it → the produced card/state. (Offline on the detector — does NOT need prod.)
2. **Judge (the oracle = OPUS).** Give the judge the RAW screen + the detector's output. It rules:
   is the card/state CORRECT — right question, ALL options, right selected, no garbage, and NO card
   on prose / no false state? Verdict + the corrected expected if wrong. The judge is ground truth;
   no hand-labeling.
3. **Triage + corpus.** If the judge says WRONG → this is a new failing case. Auto-add the captured
   snapshot + the judge's corrected `expected.json` to the corpus (doctor-validated so it's real).
   If RIGHT → optionally keep as a regression case.
4. **Fix (OPUS coding agent).** Make `detect_picker`/classifier handle the new case (parse-until-
   perfect) WITHOUT regressing the whole corpus (`testbench/run.sh` + `state-doctor` stay green).
   Structural rules only — no per-shape hacks, no legacy branches.
5. **Converge.** Repeat. Stop when K (e.g. 3) consecutive harvested rounds across all providers find
   ZERO judge-failures (the detector now matches real variety) or the rep budget is hit. Report the
   final corpus size, the failures found+fixed, and the residual (judge-flagged but unfixable) list.

This is the self-iterating, self-healing system: it DISCOVERS the long tail instead of us guessing,
the LLM-judge removes hand-labeling, and convergence is measured, not assumed.

## What each gap-mitigation maps to
- Deployment → **Phase 2** (detector into prod live path) — so the loop's gains are visible + so we
  can ALSO harvest from the live pending_prompt, not just the offline detector.
- Coverage → the **harvest** step (prompt-bank × providers) grows the corpus from reality.
- Verification → the **judge** oracle (live screen vs output) replaces "bench-green" as the gate;
  add a `live-judge` eval that drives real agents and judges the rendered card end-to-end.
- Heuristic → the **fix** step hardens the structural-signal detector; the doctor forbids bad
  snapshots; parity_test forbids native/JS drift.
- Worker quality → **opus** for judge + fixer; codex stays for mechanical capture.

## Applies to BOTH subsystems
Same loop for PICKERS (detect_picker → card) and the STATE engine (classify_timeline →
working/idle/question[subtype]): harvest live timelines, judge the predicted timeline vs the raw
session, add failing dumps, fix the classifier, converge. The state corpus shrank to 21 by dropping
invalid dumps — the loop refills it from reality with judge-labels instead of hand capture.

## Prompt-bank (seed; rotate ≥3 per surface, expand as the loop finds new ones)
- model picker: "/model"; choice: "ask me to pick a caching strategy as options, then wait";
  multi-step: "give me a multi-stage A/B/C in-TUI questionnaire across sections"; multi-select:
  "ask which phases apply (select all)"; permission: (approval-on spawn) "run the tests";
  trust/auth: spawn fresh dir / logged-out; prose-negative: "print a numbered questionnaire in text,
  answer in one line"; work: "write fib.py". (Anti-overfit: vary wording each rep.)

## Execution
- Implement as a Workflow (opus judge + opus fixer, codex harvester) — Andrey opted into the
  self-iterating multi-agent loop. Run AFTER Phase 2 lands the detector in prod (so harvest can also
  read live pending_prompt), but the loop can start now against the offline detector.
- Guardrails: never rebuild dev inside the loop (orchestrator owns it); doctor-validate every
  auto-added snapshot; every fix must keep the full corpus green; cap reps; log the residual.

<!-- revision: 1 -->
