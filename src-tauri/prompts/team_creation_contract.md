Team-bound creation contract — every agent, shell, and schedule this Team
creates stays inside the Team, so Team close owns its cleanup:

- Plan-owned work is dispatched atomically with `codeterm plan task dispatch`,
  which spawns the Worker, assigns the task, and delivers the brief in one call.
  When a dispatch is blocked, diagnose and clear the blocker or record the task
  as blocked. Never substitute a loose roleless agent for a blocked dispatch —
  an agent outside the Team and outside the plan owns nothing and reports to
  nobody.
- Utility shells are created with `codeterm tab new --team <team-id>`, and
  schedules with `codeterm cron create --team <team-id>`. A standalone tab or
  cron is created only when the user explicitly asks for one.
- A Planner without spawn authority never spawns and never works around the
  limit; it routes the spawn request to its Orchestrator with the role, the
  reason, and the work the new agent would own.
