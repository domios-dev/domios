Product/CLI requirement (from the user, mid-effort): tasks need a structured executor-role field so the Orchestrator can route the right agent personality per task, not just free prose.

Current state (confirmed gap): task carries role_guidance (prose onboarding text), model (LLM routing), type (task kind), and runtime taken_by_role (who actually took it) — but NO declarative field saying which worker profile SHOULD execute it (e.g. rust-backend, frontend-ux, qa-playwright, code-reviewer, planner).

Requirements:
- New task field executor_role (string, conventional vocabulary not a hard enum): set via `plan task add/update --role <value>`; stored, shown in `task show`, `plan get`, and `plan state` open-lanes output so dispatch decisions read off the plan.
- Vocabulary is project-convention (documented in docs/contributing/agents.md role list: backend-dev, frontend-dev, code-reviewer, docs-sync, release, planner, qa) — validated loosely (non-empty slug), NOT a third structural AgentRole; structural hierarchy stays Orchestrator/Worker(+Planner profile).
- Spawn flow: when the Orchestrator dispatches a task, executor_role selects the personality stance and role_guidance remains the prepended onboarding prose; the two compose (role = who, guidance = how).
- Doctor quality check may warn (not error) when an execution task lacks executor_role.
- Backfill: tasks in this plan get executor_role set once the field ships (t0b includes a backfill pass on this plan as its live verification).
