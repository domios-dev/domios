import { useEffect, useState } from "react";
import type { ApiConfig, Tab } from "../../lib/types";
import { daemonAuthToken, daemonHttpBase } from "../../lib/webApi";
import {
  relayRemoteTabAction,
  remoteTabStreamPath,
  requireRemoteTabRoute,
} from "../../lib/remoteTabRoute";

export function RemoteTabUnavailable({ tab }: { tab: Tab }) {
  const state = tab.remote_availability === "reconnecting" ? "Reconnecting" : "Unavailable";
  return (
    <div role="status" aria-live="polite" data-testid="remote-tab-unavailable" className="flex h-full items-center justify-center text-sm text-[var(--ct-muted-fg)]">
      {state} · @{tab.remote_peer_name ?? tab.remote_peer_id}
    </div>
  );
}

export function RemoteTabSurface({ tab, api }: { tab: Tab; api?: ApiConfig }) {
  const [output, setOutput] = useState("");
  const [chat, setChat] = useState<unknown[]>([]);
  const route = requireRemoteTabRoute(tab);

  useEffect(() => {
    const controller = new AbortController();
    void relayRemoteTabAction<{ text?: string; output?: string }>(route, "GET", "/output")
      .then((snapshot) => setOutput(snapshot?.text ?? snapshot?.output ?? ""))
      .catch(() => undefined);
    if (tab.chat_view || tab.agent_type) {
      void relayRemoteTabAction<{ messages?: unknown[] }>(route, "GET", "/chat")
        .then((snapshot) => setChat(snapshot?.messages ?? []))
        .catch(() => undefined);
    }
    const headers = new Headers();
    const token = daemonAuthToken(api);
    if (token) headers.set("Authorization", `Bearer ${token}`);
    void fetch(`${daemonHttpBase(api)}${remoteTabStreamPath(route)}`, {
      headers,
      signal: controller.signal,
    }).then(async (response) => {
      if (!response.ok || !response.body) throw new Error("Remote output stream unavailable");
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      while (!controller.signal.aborted) {
        const chunk = await reader.read();
        if (chunk.done) break;
        buffer += decoder.decode(chunk.value, { stream: true });
        let boundary = buffer.indexOf("\n\n");
        while (boundary >= 0) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          const data = frame.split("\n").find((line) => line.startsWith("data:"));
          if (data) {
            try {
              const message = JSON.parse(data.slice(5).trim()) as { tab_id?: string; output?: string };
              if (message.tab_id === route.tabId && message.output) {
                setOutput((current) => current + message.output);
              }
            } catch {}
          }
          boundary = buffer.indexOf("\n\n");
        }
      }
    }).catch(() => undefined);
    return () => controller.abort();
  }, [api, route.peerId, route.tabId, tab.chat_view, tab.agent_type]);

  return (
    <div data-testid="remote-tab-surface" className="flex h-full flex-col bg-[var(--ct-bg)]">
      {chat.length > 0 && (
        <pre data-testid="remote-chat-history" className="max-h-1/2 overflow-auto whitespace-pre-wrap p-3 text-xs">{JSON.stringify(chat, null, 2)}</pre>
      )}
      <pre data-testid="remote-pty-output" className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap p-3 font-mono text-xs">{output}</pre>
      <textarea
        aria-label={`Remote terminal input @${tab.remote_peer_name ?? tab.remote_peer_id}`}
        className="m-2 rounded border border-[var(--ct-border)] bg-transparent p-2 font-mono text-xs"
        onKeyDown={(event) => {
          if (event.key !== "Enter" || event.shiftKey) return;
          event.preventDefault();
          const target = event.currentTarget;
          const text = target.value;
          target.value = "";
          void relayRemoteTabAction(route, "POST", "/send", { text, enter: true });
        }}
      />
    </div>
  );
}
