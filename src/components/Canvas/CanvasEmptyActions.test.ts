import type { ApiConfig } from "../../lib/types";
import { createCanvasPlan } from "./CanvasEmptyActions";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function createWith(target?: { teamId?: string; managerTabId?: string | null }, created: unknown = {
  plan: {
    owner: { tab_id: "new-orchestrator" },
    team_id: "team-1",
  },
  team_id: "team-1",
}) {
  let spawns = 0;
  let params: unknown;
  await createCanvasPlan({
    api: {} as ApiConfig,
    projectPath: "/project",
    workspaceTarget: target,
    createPlan: async (_api, next) => {
      params = next;
      return created;
    },
  });
  return { spawns, params };
}

async function run() {
  const team = await createWith(
    { teamId: "team-1", managerTabId: "manager-1" },
    { plan: { owner: { tab_id: "manager-1" }, team_id: "team-1" }, team_id: "team-1" },
  );
  assert(team.spawns === 0, "team creation never spawns another orchestrator");
  assert(
    JSON.stringify(team.params) === JSON.stringify({
      title: "New plan",
      project_path: "/project",
      owner: { tab_id: "manager-1", session_id: null, title: "Team Manager" },
    }),
    "team creation owns the plan with its manager target",
  );

  let genericError: unknown;
  try {
    await createWith();
  } catch (error) {
    genericError = error;
  }
  assert(String(genericError).includes("requires a Team manager context"), "workspace-only creation is rejected");

  let mismatchError: unknown;
  try {
    await createWith({ teamId: "team-1", managerTabId: "manager-1" }, {
      plan: {
        owner: { tab_id: "manager-1" },
        team_id: "unrelated-team",
      },
      team_id: "team-1",
    });
  } catch (error) {
    mismatchError = error;
  }
  assert(
    String(mismatchError).includes("Created plan Team binding does not match"),
    "mismatched minted Team binding is a hard error",
  );
  console.log("CanvasEmptyActions tests passed");
}

void run();
