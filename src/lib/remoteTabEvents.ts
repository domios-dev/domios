import { parseCompositeTabId, remoteTabTransportHttp } from "./remoteTabTransport";
import { publishRemoteTransportState } from "./remoteTabTransportState";

const cursors = new Map<string, string>();
const resumable = new Map<string, () => void>();

export function subscribeRemoteTabEvent<T>(
  event: string,
  handler: (value: { payload: T }) => void,
): (() => void) | null {
  const prefix = "tab-chat-msg-";
  if (!event.startsWith(prefix)) return null;
  const identity = parseCompositeTabId(event.slice(prefix.length));
  if (!identity) return null;
  const remoteTabId = identity.ordinaryTabId.slice(identity.ordinaryTabId.indexOf("/") + 1);
  const params = new URLSearchParams({
    peer: identity.peerId,
    path: "/api/events/stream",
    tab: identity.ordinaryTabId,
  });
  const cursor = cursors.get(identity.ordinaryTabId);
  if (cursor) params.set("since", cursor);
  const abort = new AbortController();
  const connect = () => {
    if (!abort.signal.aborted) subscribeRemoteTabEvent(event, handler);
  };
  resumable.set(event, connect);
  const { base, token } = remoteTabTransportHttp();
  void fetch(`${base}/api/mesh/relay-stream?${params.toString()}`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    signal: abort.signal,
  }).then(async (response) => {
    if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (!abort.signal.aborted) {
      const chunk = await reader.read();
      if (chunk.done) break;
      buffer += decoder.decode(chunk.value, { stream: true });
      let boundary = buffer.indexOf("\n\n");
      while (boundary >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = frame.split("\n").find((line) => line.startsWith("data:"));
        if (data) {
          const message = JSON.parse(data.slice(5).trim()) as { event_type?: string; data?: Record<string, unknown>; boot?: string; seq?: number };
          if (message.boot && typeof message.seq === "number") cursors.set(identity.ordinaryTabId, `${message.boot}:${message.seq}`);
          if (message.event_type === `${prefix}${remoteTabId}` && message.data) {
            handler({ payload: { ...message.data, tab_id: identity.ordinaryTabId } as T });
          }
        }
        boundary = buffer.indexOf("\n\n");
      }
    }
    if (!abort.signal.aborted) publishRemoteTransportState(identity.ordinaryTabId, "reconnecting");
  }).catch(() => { if (!abort.signal.aborted) publishRemoteTransportState(identity.ordinaryTabId, "unavailable"); });
  return () => { abort.abort(); resumable.delete(event); };
}

export function resumeRemoteTabEvents(): void {
  const pending = [...resumable.values()];
  resumable.clear();
  for (const connect of pending) connect();
}
