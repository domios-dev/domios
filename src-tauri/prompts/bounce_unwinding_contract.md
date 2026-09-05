# Bounce-unwinding protocol

- At the third consecutive stagnant bounce (no changed executable surfaces, no distinct oracle), the strategy warning's `decisions` field carries the task's recorded `decision`-kind strides — the same milestones `codeterm plan record <plan> --task <task> --step decision:<text>` writes. Re-read them (`codeterm task show <task>`) before any other repo surface; an empty set means no decision was ever recorded for this task, not that none applied.
- Treat a bounce as evidence that the task, plan, oracle, or routing may be wrong. Diagnose it instead of requesting a blind retry.
- Research is repo-first and bounded. Start with local code, touched files, plan skills, existing evals, and executable fixtures. External research is allowed only when those surfaces cannot answer the one missing fact that blocked the bounce.
- Start a missing-symbol or missing-fact check with `rg -n "<symbol>" <named repo surfaces>`, then inspect `codeterm task show <task>` and the relevant `codeterm plan section show <plan> <section>` or `codeterm plan skill show <plan> <skill>` before any external lookup.
- Preserve the immutable baseline: copy the accepted target, oracle, and pass/fail bar verbatim. Unless the user explicitly changes them, never weaken an oracle, relabel failure as success, broaden the proving command, or rewrite acceptance text to fit the result.
- Rank evidence by authority and applicability: repository source and executable fixtures first, official docs/specs second, narrower external sources last. Record why each source applies, not merely that it exists.
- Synthesize every bounce with explicit keep/discard/supersede decisions. Keep the valid baseline and evidence, discard disproved theories or noise, and supersede only stale guidance with a better-cited replacement.
- Use this complete record. Do not omit a field or replace the numeric budget with a vague limit:
  ```text
  Trigger: <failed gate, repeated finding, or contradiction>
  Question: <one missing fact or decision>
  Hypothesis: <falsifiable explanation>
  Baseline target: <accepted target, verbatim>
  Baseline oracle: <accepted proving command/observation, verbatim>
  Baseline pass/fail: <accepted result, verbatim>
  Evidence: repo/fixture <path> — <why applicable>; official source <if needed> — <why applicable>
  Budget: inspect at most 5 repo surfaces; consult at most 1 external source
  Stop rule: <the observation that ends research and returns to synthesis>
  Keep: <still-valid evidence or baseline>
  Discard: <disproved theory or noisy evidence>
  Supersede: <stale guidance and its better-cited replacement, or none>
  Route: <stride/plan edit, `codeterm plan skill new|edit`, `codeterm orchestrator learn`, or `codeterm plan retrospective <plan> --body "..."`>
  Fixture: <exact command or deterministic fixture that proves the route>
  Follow-up: none, or <one distinct action and ownership boundary>
  ```
- Enforce the recorded budget and stop rule: inspect at most 5 named repo surfaces, consult at most 1 external source for the Question, then return to synthesis at the stated observation. "Investigate more" is not a valid task.
- Gate follow-ups. Create one only for a distinct next action or ownership boundary, reuse an open task for a matching Question, and never create duplicate or parallel copies. Every follow-up copies the Question, Hypothesis, Baseline oracle, Budget, Stop rule, and Fixture and retains an executable fixture or oracle.
- Route outcomes to existing homes: task-local truth and evidence to a stride or plan edit; reusable verification or research procedure to `codeterm plan skill new` or `codeterm plan skill edit`; orchestration policy to `codeterm orchestrator learn`; end-of-effort judgment quality to `codeterm plan retrospective <plan> --body "..."`. Do not invent a runtime store.
