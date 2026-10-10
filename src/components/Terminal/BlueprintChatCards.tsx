import { useMemo, type CSSProperties } from "react";
import { Workflow } from "lucide-react";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { pipelineModel } from "../Automation/pipelineModel";
import { requestOpenJob } from "../Jobs/openJobEvent";
import { useWorkspaceGraphs } from "../Jobs/useWorkspaceGraphs";

const NO_SOURCES: readonly string[] = [];

export function BlueprintChatCards({ tabId, workspaceId }: { tabId: string; workspaceId: string | null }) {
  const sources = useMemo(() => workspaceId ? [workspaceId] : NO_SOURCES, [workspaceId]);
  const graphs = useWorkspaceGraphs(hostAutomationAdapter, `blueprint-chat:${tabId}`, sources);
  const blueprints = useMemo(() => graphs.flatMap((graph) => (graph.graph?.blueprints ?? [])
    .filter((blueprint) => blueprint.owner?.tab_id === tabId)
    .map((blueprint) => ({ blueprint, workspaceId: graph.attachment.workspace_id, pipeline: pipelineModel(blueprint) }))), [graphs, tabId]);
  if (blueprints.length === 0) return null;
  return <section aria-label="Blueprint activity" className="my-2 space-y-2">
    {blueprints.map(({ blueprint, workspaceId: ownerWorkspaceId, pipeline }) => {
      const chain = pipeline.nodes.map((node) => node.kind === "result" ? node.targetSummary ?? "Result" : node.title).join(" → ");
      return <article key={blueprint.id} className="ct-card px-3 py-2" style={{ "--ct-card-tone": "var(--ct-tone-blueprint)" } as CSSProperties}>
        <div className="flex items-center gap-2 text-xs text-[var(--ct-muted-fg)]"><Workflow size={13} aria-hidden="true" className="text-[var(--ct-card-tone)]" /><span>Blueprint · {blueprint.state}</span></div>
        <h3 className="mt-1 truncate text-sm font-semibold" title={blueprint.name}>{blueprint.name}</h3>
        <p className="truncate text-xs text-[var(--ct-muted-fg)]" title={chain}>{chain}</p>
        <button type="button" className="mt-1 text-xs font-medium text-[var(--ct-accent)] underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" aria-label={`Open Blueprint ${blueprint.name}`} onClick={() => requestOpenJob({ workspaceId: ownerWorkspaceId, target: { teamId: blueprint.owner?.team_id ?? null, blueprintId: blueprint.id } })}>Open Blueprint</button>
      </article>;
    })}
  </section>;
}
