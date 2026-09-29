import { buildTeamActions } from "./teamActions";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function run() {
  const calls: Array<{ command: string; args: unknown }> = [];
  const inventory = { removed: [], kept: [], orphan_branches: ["lane/inventory-token-4471"] };
  const actions = buildTeamActions({
    invoke: async (command, args) => {
      calls.push({ command, args });
      return command === "mux_close_team" ? { team: "team-1", inventory } : undefined;
    },
  });

  await actions.onRenameTeam("team-1", "Renamed team", "O1");

  assert(calls.length === 1, "rename invokes exactly once");
  assert(calls[0]?.command === "mux_rename_team", "rename uses the bridge command");
  assert(
    JSON.stringify(calls[0]?.args) === JSON.stringify({
      teamId: "team-1",
      name: "Renamed team",
      callerTabId: "O1",
    }),
    "rename passes exact team id, name, and acting Tab identity",
  );
  assert(!JSON.stringify(calls[0]?.args).includes("focused_tab_id"), "rename never sends focus state");
  assert(actions.onAddTeamMember, "staffing is enabled when the bridge is available");
  const callsBeforeStaffing = calls.length;
  await actions.onAddTeamMember("team-1", "worker", "registry-provider", "manager-tab", "model-2");
  assert(calls.length === callsBeforeStaffing + 1, "staffing invokes exactly once");
  const staffingCall = calls[calls.length - 1];
  assert(staffingCall?.command === "mux_add_team_member", "staffing uses the bridge command");
  assert(
    JSON.stringify(staffingCall?.args) === JSON.stringify({
      teamId: "team-1",
      role: "worker",
      provider: "registry-provider",
      model: "model-2",
      callerTabId: "manager-tab",
    }),
    "staffing uses the selected manager as caller identity and keeps UI-only ids off the wire",
  );
  await actions.onAddTeamMember("team-1", "cartographer", "registry-provider", "manager-tab");
  assert(
    JSON.stringify(calls[calls.length - 1]?.args) === JSON.stringify({
      teamId: "team-1",
      role: "cartographer",
      provider: "registry-provider",
      callerTabId: "manager-tab",
    }),
    "Cartographer uses the same role-kind staffing contract as other Team members",
  );
  await actions.onAddTeamMember("team-1", "worker", "registry-provider", "manager-tab");
  const noModelCall = calls[calls.length - 1];
  assert(
    JSON.stringify(noModelCall?.args) === JSON.stringify({
      teamId: "team-1",
      role: "worker",
      provider: "registry-provider",
      callerTabId: "manager-tab",
    }),
    "model is absent when the descriptor does not require one",
  );
  assert(!JSON.stringify(noModelCall?.args).includes("parentTabId"), "staffing payload omits parent Tab ids");
  assert(!JSON.stringify(noModelCall?.args).includes("providerId"), "staffing payload uses only the bridge provider key");
  await actions.onAddTeamMember("team-1", "orchestrator", "codex", "manager-tab");
  assert(
    JSON.stringify(calls[calls.length - 1]?.args) === JSON.stringify({
      teamId: "team-1",
      role: "orchestrator",
      provider: "codex",
      callerTabId: "manager-tab",
    }),
    "orchestrator staffing reaches the same Team member bridge",
  );
  const callsBeforeClose = calls.length;
  const closed = await actions.onCloseTeam("team-1");
  assert(closed === inventory, "close returns the daemon lane inventory");
  assert(calls.length === callsBeforeClose + 1, "close invokes exactly once");
  const closeCall = calls[calls.length - 1];
  assert(closeCall?.command === "mux_close_team", "close uses the bridge command");
  assert(
    JSON.stringify(closeCall?.args) === JSON.stringify({ teamId: "team-1", forceBusy: true }),
    "close is a human UI action and sends no agent authority",
  );

  const rejected = buildTeamActions({
    invoke: async () => {
      throw new Error("Team manager permission denied (403)");
    },
  });
  await rejected.onRenameTeam("team-1", "Rejected rename", "O1")
    .then(
      () => assert(false, "403 rename rejection must be surfaced"),
      (error: Error) => assert(error.message.includes("403"), "rename 403 remains actionable"),
    );
  await rejected.onAddTeamMember("team-1", "planner", "registry-provider", "manager-tab")
    .then(
      () => assert(false, "403 staffing rejection must be surfaced"),
      (error: Error) => assert(error.message.includes("403"), "403 error remains actionable"),
    );

  console.log("team actions tests passed");
}

void run();
