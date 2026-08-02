import { parseCompositeTabId, remoteTabTransportHttp } from "./remoteTabTransport";

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
  const abort = new AbortController();
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
          const message = JSON.parse(data.slice(5).trim()) as { event_type?: string; data?: Record<string, unknown> };
          if (message.event_type === `${prefix}${remoteTabId}` && message.data) {
            handler({ payload: { ...message.data, tab_id: identity.ordinaryTabId } as T });
          }
        }
        boundary = buffer.indexOf("\n\n");
      }
    }
  }).catch(() => undefined);
  return () => abort.abort();
}
