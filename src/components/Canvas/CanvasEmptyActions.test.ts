import type { ApiConfig } from "../../lib/types";
import { createCanvasPlan } from "./CanvasEmptyActions";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function createWith(target?: { teamId?: string; managerPaneId?: string | null }, created: unknown = {
  plan: {
    owner: { pane_id: "new-orchestrator" },
    team_id: "team-1",
  },
  team_id: "team-1",
}) {
  let spawns = 0;
  let params: unknown;
  let spawnOptions: unknown;
  await createCanvasPlan({
    api: {} as ApiConfig,
    projectPath: "/project",
    workspaceId: "workspace-1",
    providerId: "registry-provider",
    workspaceTarget: target,
    spawnOrchestrator: async (_api, options) => {
      spawns += 1;
      spawnOptions = options;
      return "new-orchestrator";
    },
    createPlan: async (_api, next) => {
      params = next;
      return created;
    },
  });
  return { spawns, params, spawnOptions };
}

async function run() {
  const team = await createWith(
    { teamId: "team-1", managerPaneId: "manager-1" },
    { plan: { owner: { pane_id: "manager-1" }, team_id: "team-1" }, team_id: "team-1" },
  );
  assert(team.spawns === 0, "team creation never spawns another orchestrator");
  assert(
    JSON.stringify(team.params) === JSON.stringify({
      title: "New plan",
      project_path: "/project",
      owner: { pane_id: "manager-1", session_id: null, title: "Team Manager" },
    }),
    "team creation owns the plan with its manager target",
  );

  const generic = await createWith();
  assert(generic.spawns === 1, "generic creation retains orchestrator spawn behavior");
  assert(
    JSON.stringify(generic.spawnOptions) === JSON.stringify({ providerId: "registry-provider", workspaceId: "workspace-1" }),
    "generic creation carries provider and workspace provenance to the spawn edge",
  );
  assert(
    JSON.stringify(generic.params) === JSON.stringify({
      title: "New plan",
      project_path: "/project",
      owner: { pane_id: "new-orchestrator", session_id: null, title: "Canvas Orchestrator" },
    }),
    "generic creation uses its spawned orchestrator",
  );

  let mismatchError: unknown;
  try {
    await createWith(undefined, {
      plan: {
        owner: { pane_id: "new-orchestrator" },
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
