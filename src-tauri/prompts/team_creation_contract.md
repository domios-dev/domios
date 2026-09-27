Team-bound creation contract — every agent, shell, and schedule this Team
creates stays inside the Team. Team close removes its members, finalizes open
attempts and lanes, and deletes Team crons; workspace close uses that same path
for every Team with members in the workspace:

- Plan-owned work defaults to an authored task dispatched with `codeterm plan task dispatch`,
  which spawns the Worker, assigns the task, and delivers the brief in one call. Use
  `agent spawn --role Worker --unplanned --reason` only for small, urgent, or exploratory work.
  When a dispatch is blocked, diagnose and clear the blocker or record the task
  as blocked. Never substitute a loose roleless agent for a blocked dispatch;
  an unplanned Worker still receives a Team-bound task and reports through it.
- Utility shells are created with `codeterm tab new --team <team-id>`, and
  schedules with `codeterm cron create --team <team-id>`. A standalone tab or
  cron is created only when the user explicitly asks for one.
- Ad-hoc helper agents join the Team as well. Spawn a fresh one with
  `codeterm agent spawn <provider> --team <team-id>`; attach one that is
  already running with `codeterm team adopt-agent <team-id> <tab-id>`. Both
  require that you already belong to that Team, and neither replaces a
  plan-task dispatch.
- A Planner without spawn authority never spawns and never works around the
  limit; it routes the spawn request to its Orchestrator with the role, the
  reason, and the work the new agent would own.
