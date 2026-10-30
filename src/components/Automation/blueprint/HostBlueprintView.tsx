import { useCallback, useMemo } from "react";
import { orchestrationTransport } from "../../../lib/transport/orchestrationRuntime";
import { BackPill } from "../../shared/BackPill";
import { InlineErrorNotice } from "../../shared/ErrorNotice";
import { pipelineModel } from "../pipelineModel";
import { automationKey, type AutomationAttachment, type AutomationGraph, type AutomationGraphAdapter, type AutomationIdentity } from "../types";
import { PersistedBlueprintView as BlueprintView } from "../../Navigation/PersistedBlueprintView";
import { watchRuns, withRetainedRuns } from "./runEvidence";
import { triggerRuntime } from "./triggerModel";

const STOPPABLE = new Set(["running", "active", "idle", "held", "finishing"]);

/** Opens one Blueprint of a workspace graph, whatever its kind, in the single Blueprint view. */
export function HostBlueprintView({ graph, attachment, adapter, refresh, blueprintId, initialRunId, backLabel, onBack, headingLevel, fill }: {
  graph: AutomationGraph; attachment: AutomationAttachment; adapter: AutomationGraphAdapter; refresh: () => Promise<unknown>;
  blueprintId: string; initialRunId?: string; backLabel: string; onBack: () => void; headingLevel?: 2 | 3; fill?: boolean;
}) {
  const snapshot = graph.blueprints?.find((blueprint) => blueprint.id === blueprintId);
  const base = useMemo(() => snapshot ? withRetainedRuns(pipelineModel(snapshot), watchRuns(graph, snapshot)) : null, [graph, snapshot]);
  const workspaceId = graph.workspace_id;
  const teamId = base?.owner.team_id ?? null;
  const identity: AutomationIdentity | null = base ? graph.scenarios.find((scenario) => scenario.root.id === blueprintId)?.root
    ?? { kind: "blueprint", id: blueprintId, workspace_id: workspaceId, team_id: teamId, owner_tab_id: base.owner.tab_id } : null;
  const root = identity ? graph.nodes.find((node) => automationKey(node.identity) === automationKey(identity)) : undefined;
  const pipeline = useMemo(() => base ? { ...base, triggerRuntime: triggerRuntime(root) } : null, [base, root]);
  const history = useCallback(() => orchestrationTransport().blueprint_viewer_history({ workspaceId, teamId, id: blueprintId }), [workspaceId, teamId, blueprintId]);
  if (!pipeline || !identity) return <div className="flex flex-col items-start gap-3 p-4">
    <BackPill label={backLabel} onBack={onBack} />
    <InlineErrorNotice error={{ code: "blueprint_unavailable", raw: "" }} />
  </div>;
  const stop = async () => {
    await adapter.stop(attachment, identity);
    await refresh();
  };
  return <BlueprintView key={blueprintId} navigationScope={JSON.stringify([workspaceId, teamId, blueprintId])} pipeline={pipeline} initialRunId={initialRunId} backLabel={backLabel} onBack={onBack}
    exportTarget={{ workspaceId, teamId: identity.team_id, id: blueprintId }} loadHistory={history}
    onStop={stop} canStop={STOPPABLE.has(pipeline.state) && (root?.stop.allowed ?? true)} headingLevel={headingLevel} fill={fill} />;
}
