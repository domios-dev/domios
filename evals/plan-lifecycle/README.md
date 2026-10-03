# Plan Lifecycle Eval

Real-daemon lifecycle bench for the Phase 1 canvas plan surface.

## What It Covers

- Create a plan from a spec file and assert sections are present.
- Add a second section and a task linked to multiple sections.
- Assign, report to the task, and assert a milestone was appended.
- Close the task, create a loop, trigger budget teaching, archive, disown, and list unowned plans.
- Assert teaching errors for bad milestone kind, bad task status, unknown plan id, dash-leading text, and loop budget exhaustion.

## Run

Start a dev daemon rebuilt from the current branch, then run:

```bash
./evals/plan-lifecycle/run.sh
```

The script sources `evals/_lib/lib.sh` and uses the `dc` wrapper for every Domios CLI call, so it is locked to dev on port 7686 and cannot fall back to prod.

<!-- revision: 2 -->
