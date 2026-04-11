import type { ApiConfig, GraphWorkerInfo } from "../../lib/types";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";

interface Props {
  worker: GraphWorkerInfo;
  api: ApiConfig;
  onClose: () => void;
}

export function WorkerLogPopup({ worker, api, onClose }: Props) {
  const intent = worker.intent.replace(/^extract:/, "");
  const subtitle = [worker.agent_provider, worker.model]
    .filter(Boolean)
    .join(" / ");
  return (
    <AgentRunLogPopup
      api={api}
      paneId={worker.pane_id}
      title={intent}
      status={worker.status}
      color={worker.color}
      subtitle={subtitle || undefined}
      report={worker.report ?? undefined}
      onClose={onClose}
    />
  );
}
