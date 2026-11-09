import type { StreamEnd, TransportClient } from "../../lib/transportClient";

export interface AgentRunOutputHandlers {
  output(text: string): void;
  connected(value: boolean): void;
  ended(end: StreamEnd, offset: number): void;
}

export function subscribeAgentRunOutput(
  client: TransportClient,
  tabId: string,
  signal: AbortSignal,
  handlers: AgentRunOutputHandlers,
): () => void {
  let offset = 0;
  let failures = 0;
  let stopped = false;
  let close = () => {};
  const connect = () => {
    if (stopped || signal.aborted) return;
    close = client.subscribe(`/api/tabs/${encodeURIComponent(tabId)}/output/stream?from=${offset}`, (_name, data) => {
      if (stopped || signal.aborted || typeof data !== "object" || data === null) return;
      const frame = data as { output?: unknown; offset?: unknown };
      if (typeof frame.output !== "string" || typeof frame.offset !== "number"
        || !Number.isSafeInteger(frame.offset) || frame.offset <= offset) return;
      handlers.output(frame.output);
      offset = frame.offset;
      failures = 0;
    }, {
      signal, raw: true, local: true, requireEventStream: true,
      onReady: () => { if (!stopped && !signal.aborted) handlers.connected(true); },
      onEnd: (end) => {
        if (stopped || signal.aborted) return;
        handlers.connected(false);
        if (end.reason === "http" || end.reason === "abort" || ++failures > 3) handlers.ended(end, offset);
        else queueMicrotask(connect);
      },
    });
  };
  connect();
  return () => { stopped = true; close(); };
}
