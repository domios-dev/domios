import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import { createTransportClient } from "../../lib/transportClient";
import { restoreStageView } from "../../lib/restoreStageView";
import type { Tab } from "../../lib/types";
import { RestoreStatePanel } from "./RestoreStatePanel";
import { useWakeOnOpen } from "./useWakeOnOpen";

it("keeps automatic active-tab restore loading until the absent wake route is installed", async () => {
  let subscribed!: () => void;
  const opening = new Promise<void>(resolve => { subscribed = resolve; });
  let ready!: () => void;
  const readiness = new Promise<void>(resolve => { ready = resolve; });
  let attempts = 0;
  const fetch = vi.fn<typeof globalThis.fetch>().mockImplementation(async input => {
    if (String(input).endsWith("/api/startup/events")) {
      subscribed();
      await readiness;
      return new Response('event: startup-state\ndata: {"state":"ready"}\n\n', {
        headers: { "Content-Type": "text/event-stream" },
      });
    }
    return ++attempts === 1
      ? Response.json({ error: { code: "starting", subsystem: "service", request_executed: false } }, { status: 503 })
      : Response.json({ accepted: true });
  });
  const client = createTransportClient({ origin: "http://127.0.0.1:17786", token: "fixture-token", fetch });
  let wake!: Promise<unknown>;
  __setInvokeForTest(<T,>(command: string): Promise<T> => {
    if (command !== "wake_tab") return Promise.resolve({} as T);
    const result = client.json<T>("POST", "/api/tabs/startup-active-fixture/wake");
    wake = result;
    return result;
  });
  const tab = { id: "startup-active-fixture", frozen: true, agent_type: "fixture", agent_session_id: "opaque" } as Tab;
  function ActiveTab() {
    const state = useWakeOnOpen(tab, true);
    return <RestoreStatePanel view={restoreStageView(tab, state)} />;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<StrictMode><ActiveTab /></StrictMode>));
    await act(async () => { await opening; });
    expect(attempts).toBe(1);
    expect(host.querySelector('[role="progressbar"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector("details")).toBeNull();
    await act(async () => { ready(); await wake; });
    expect(attempts).toBe(2);
    expect(host.querySelector('[role="progressbar"]')).not.toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
  } finally {
    ready();
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
});
