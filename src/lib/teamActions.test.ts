import { buildTeamActions } from "./teamActions";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function run() {
  const calls: Array<{ command: string; args: unknown }> = [];
  const actions = buildTeamActions({
    callerTabId: () => "acting-pane",
    invoke: async (command, args) => {
      calls.push({ command, args });
    },
  });

  await actions.onRenameTeam("team-1", "Renamed team");

  assert(calls.length === 1, "rename invokes exactly once");
  assert(calls[0]?.command === "mux_rename_team", "rename uses the bridge command");
  assert(
    JSON.stringify(calls[0]?.args) === JSON.stringify({
      teamId: "team-1",
      name: "Renamed team",
      callerTabId: "acting-pane",
    }),
    "rename passes exact team id, name, and acting pane identity",
  );
  assert(actions.onAddTeamMember, "staffing is enabled when the bridge is available");
  const callsBeforeStaffing = calls.length;
  await actions.onAddTeamMember("team-1", "worker", "registry-provider", "manager-pane");
  assert(calls.length === callsBeforeStaffing + 1, "staffing invokes exactly once");
  const staffingCall = calls[calls.length - 1];
  assert(staffingCall?.command === "mux_add_team_member", "staffing uses the bridge command");
  assert(
    JSON.stringify(staffingCall?.args) === JSON.stringify({
      teamId: "team-1",
      role: "worker",
      provider: "registry-provider",
      callerTabId: "manager-pane",
    }),
    "staffing uses the selected manager as caller identity and keeps UI-only ids off the wire",
  );

  const rejected = buildTeamActions({
    callerTabId: () => "acting-pane",
    invoke: async () => {
      throw new Error("Team manager permission denied (403)");
    },
  });
  await rejected.onAddTeamMember("team-1", "planner", "registry-provider", "manager-pane")
    .then(
      () => assert(false, "403 staffing rejection must be surfaced"),
      (error: Error) => assert(error.message.includes("403"), "403 error remains actionable"),
    );

  console.log("team actions tests passed");
}

void run();
