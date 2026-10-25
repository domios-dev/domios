# Chair decisions before the pick

These clarify execution without changing the preregistered selection rule.

- L12 at 512 tokens is the primary baseline; the task's 128 assumption is superseded. This decision is already in the immutable preregistration.
- Scratch-only adapters select one global provider and read its identity; mem-bench must explicitly re-embed the fixed seed corpus and assert vectors exist. C10 and scoring stay unchanged.
- `ReembedReport` currently contains items, entities and errors. The actual background switch returns `SwitchPlan {items,entities,docs}` and emits `ReembedProgress`; it does not use that report. The Chair approved proving switch counts through those actual contracts, plus a separate real `reembed_bulk` report for items/entities. Docs count is explicitly reported from SwitchPlan. Do not change the switch/report architecture to match the brief wording.
- If the default changes, pin `legacy_identity()` to the L12 descriptor so an unstamped historical store is identified correctly; add a focused regression proving re-embedding instead of reuse. This limited change is approved by the Chair.

The eval Rust sources have their own non-published Cargo manifest so the repository formatter can resolve their edition. Measurement builds register them as examples only on the never-delivered scratch branch, retaining the existing app workspace dependency versions and per-worktree build target.

Final owner narrowing before the pick supersedes the earlier execution plan: keep L12; drop mem-bench, snapshot recall and subject sessions; delete worker snapshot copies; commit the decision and then confirm L12 once on b4. The discarded harness sources and extra migration fixture were removed. See DECISION.md for the decision and deferred legacy-identity follow-up.
