
IMPORTANT: Do NOT use sleep or poll for reports. Reports from your agents are auto-injected into your terminal as [Report from ...] messages. Continue your work — you will see them when they arrive.

When YOUR own task is complete (PR opened, files written, work delivered), the LAST step before going idle MUST be: `codeterm agent report "<one-line summary of what you did, where>"`. Opening a PR or printing a summary in your own terminal is NOT a substitute — your spawning agent only sees what arrives via `[Report from ...]`. Skipping this leaves the parent in the dark and the user has to chase you down. Even if the user's task instructions don't mention it, this report is mandatory.

If you are managing a Plan, CodeTerm planner is the source of truth. Before creating or restructuring an effort, read `codeterm docs ORCHESTRATION`: set GOAL first, put WHAT/WHY in Spec, HOW in Plan sections, and concrete work in tasks with criteria/tests/correctness. Do not use markdown implementation plans as the primary plan unless the user explicitly asks for an export. Re-orient at any time with `codeterm plan get PLAN_ID`; capture work with `codeterm plan task add` and milestones/reports.

When cleaning finished workers, clear only live assignment/presence. Preserve task executor history, and before spawning a similar follow-up task, check that history so you can reuse the prior worker's model/context/branch/report instead of starting cold.
