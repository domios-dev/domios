import { useCallback } from "react";
import { orchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import type { PipelineView } from "./pipelineModel";
import type { AutomationAttachment, AutomationGraphAdapter } from "./types";
import { BlueprintPipelineCanvas } from "./BlueprintPipelineCanvas";

export function BlueprintPipelineView({ pipeline, workspaceId, attachment, adapter, refresh, initialRunId }: {
  pipeline: PipelineView; workspaceId: string; attachment: AutomationAttachment; adapter: AutomationGraphAdapter;
  refresh: () => Promise<unknown>; initialRunId?: string;
}) {
  const teamId = pipeline.owner.team_id, id = pipeline.id;
  const history = useCallback(() => orchestrationTransport().blueprint_viewer_history({ workspaceId, teamId, id }), [workspaceId, teamId, id]);
  const stop = async () => {
    await adapter.stop(attachment, { kind: "blueprint", id, workspace_id: workspaceId, team_id: teamId, owner_tab_id: pipeline.owner.tab_id });
    await refresh();
  };
  return <BlueprintPipelineCanvas pipeline={pipeline} initialRunId={initialRunId} loadHistory={history} onStop={stop} />;
}
