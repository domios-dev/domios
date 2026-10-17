import type { CronJob, MultiplexerState } from "../../lib/types";
import type { TransportClient } from "../../lib/transportClient";
import { createOrchestrationTransport } from "../../lib/transport/orchestration";
import { blueprintScenarioJobs } from "../Automation/scenarios";
import { ALL_JOBS_SCOPE } from "../Automation/types";
import type { MeshInventoryView } from "../MeshPanel/servicesView";
import { blueprintJobs, mergeJobs, NO_JOBS_FILTER } from "./jobsModel";

export async function readPeerJobs(client: TransportClient, signal: AbortSignal) {
  const transport = createOrchestrationTransport(client);
  const [snapshot, crons, inventory] = await Promise.all([
    client.json<MultiplexerState>("GET", "/api/mux/state", undefined, { signal }),
    client.json<CronJob[]>("GET", "/api/crons", undefined, { signal }),
    client.json<MeshInventoryView>("GET", "/api/mesh/inventory", undefined, { signal }).catch(() => null),
  ]);
  if (!Array.isArray(snapshot.workspaces) || !Array.isArray(crons)) throw new Error("Jobs unavailable");
  const graphs = await Promise.all(snapshot.workspaces.map(async (workspace) => {
    try {
      const graph = await transport.automation_graph_workspace_snapshot({ workspaceId: workspace.id });
      return { graph, attachment: { connection_id: "peer-jobs", workspace_id: workspace.id, team_id: ALL_JOBS_SCOPE } };
    } catch { return null; }
  }));
  signal.throwIfAborted();
  return {
    workspaces: snapshot.workspaces,
    teams: snapshot.teams ?? [],
    resources: inventory?.resources ?? [],
    jobs: mergeJobs(NO_JOBS_FILTER, crons, graphs.flatMap((entry) => entry ? blueprintScenarioJobs(entry.graph, entry.attachment) : []), graphs.flatMap((entry) => entry ? blueprintJobs(entry.graph) : [])),
    partial: graphs.some((entry) => !entry),
  };
}

export type PeerJobsData = Awaited<ReturnType<typeof readPeerJobs>>;
