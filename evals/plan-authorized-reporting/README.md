# Plan-authorized reporting oracle

This canonical-only drill verifies that an Orchestrator can grant one exact,
task-less QA identity permission to append immutable Plan evidence without
granting task ownership, task transitions, Planner edits, or release authority.

Start an exact-SHA dev instance, then run from Git Bash:

```bash
bash evals/plan-authorized-reporting/run.sh --check
```

The script sources `evals/_lib/lib.sh` and uses only its dev-locked `dc`
wrapper. It creates unique temporary workspaces, a Team and Plan, an owner,
an authorized QA tab, and foreign, unbound, rebound, and task-bound refusal
fixtures. The event subscription is ready before the accepted report is sent.

Success prints PASS rows for authorized persistence and owner delivery, plus
foreign-tab, unbound-tab, stale/rebound-identity, and task-bound refusal. Every
refusal must leave Plan strides and task state unchanged. Cleanup removes only
resources whose unique cwd or create response established ownership, and any
setup, assertion, event, or cleanup failure exits nonzero.
