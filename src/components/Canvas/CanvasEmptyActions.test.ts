import type { ApiConfig } from "../../lib/types";
import { createCanvasPlan } from "./CanvasEmptyActions";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function createWith(target?: { teamId?: string; managerPaneId?: string | null }) {
  let spawns = 0;
  let params: unknown;
  await createCanvasPlan({
    api: {} as ApiConfig,
    projectPath: "/project",
    workspaceTarget: target,
    spawnOrchestrator: async () => {
      spawns += 1;
      return "new-orchestrator";
    },
    createPlan: async (_api, next) => {
      params = next;
    },
  });
  return { spawns, params };
}

async function run() {
  const team = await createWith({ teamId: "team-1", managerPaneId: "manager-1" });
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
    JSON.stringify(generic.params) === JSON.stringify({
      title: "New plan",
      project_path: "/project",
      owner: { pane_id: "new-orchestrator", session_id: null, title: "Canvas Orchestrator" },
    }),
    "generic creation uses its spawned orchestrator",
  );
  console.log("CanvasEmptyActions tests passed");
}

void run();
