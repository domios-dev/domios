import { useEffect, useState } from "react";
import type { ApiConfig, GraphWorkerInfo } from "../../lib/types";
import { AgentRunLogPopup } from "../AgentRun/AgentRunLogPopup";
import { authHeaders, baseUrl } from "./memoryApi";

const TERMINAL = new Set(["done", "error", "orphaned"]);

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
  const isTerminal = TERMINAL.has(worker.status);

  // For completed workers: fetch the cleaned log text instead of connecting
  // to the raw PTY stream. The Claude parser can't parse headless terminal
  // output, so SSE would only produce collapsed "raw shell output" lines.
  const [logText, setLogText] = useState<string | null>(null);
  useEffect(() => {
    if (!isTerminal) return;
    fetch(`${baseUrl(api)}/api/mem/workers/${encodeURIComponent(worker.worker_id)}/log`, {
      headers: authHeaders(api),
    })
      .then((r) => (r.ok ? r.text() : Promise.reject(r.statusText)))
      .then(setLogText)
      .catch(() => {});
  }, [api, worker.worker_id, isTerminal]);

  return (
    <AgentRunLogPopup
      api={api}
      paneId={isTerminal ? undefined : worker.pane_id}
      title={intent}
      status={worker.status}
      color={worker.color}
      subtitle={subtitle || undefined}
      report={worker.report ?? undefined}
      fallbackText={logText}
      onClose={onClose}
    />
  );
}
