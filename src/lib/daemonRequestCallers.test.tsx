import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { configureDaemonTransport } from "./daemonBootstrap";
import { clearRelayTarget, setRelayTarget } from "./webApi";
import { listen } from "./bridge";
import { defaultConfig } from "../hooks/useConfig";
import { AgentRunLogPopup } from "../components/AgentRun/AgentRunLogPopup";
import { useActivityPulses, type UseActivityPulsesResult } from "../components/MemoryPanel/useActivityPulses";
import { LogsTab } from "../components/Settings/LogsTab";
import { SecretsTab } from "../components/Settings/SecretsTab";
import { rebindTabSession } from "../components/Terminal/sessionBindingApi";
import type { GraphWorkerInfo } from "./types";

vi.mock("./bridge", async importOriginal => ({
  ...await importOriginal<typeof import("./bridge")>(),
  IS_WEB: false,
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../store/multiplexer/hooks", () => ({ useMuxState: () => ({ workspaces: [] }), useMultiplexer: () => ({ selectTabByTab: async () => {} }) }));
vi.mock("../components/AgentRun/useAgentRunStream", () => ({
  useAgentRunStream: () => ({ events: [], rawLines: [], connected: false, error: null }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let request: ReturnType<typeof installRequest>;

function installRequest() {
  const client = configureDaemonTransport({ origin: window.location.origin, token: "caller-token" });
  return vi.spyOn(client, "request").mockImplementation(async () => Response.json({}));
}

async function render(node: ReactNode) {
  await act(async () => root.render(node));
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  request = installRequest();
  setRelayTarget("foreign-peer", "Foreign");
  vi.stubGlobal("fetch", vi.fn(() => { throw new Error("Raw daemon fetch bypass"); }));
  vi.mocked(listen).mockImplementation(async () => () => {});
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  clearRelayTarget();
  expect(globalThis.fetch).not.toHaveBeenCalled();
  expect(request.mock.calls.every(([, , local]) => local === true)).toBe(true);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("local daemon request callers", () => {
  it("loads a terminal worker's text log with an encoded id", async () => {
    const worker: GraphWorkerInfo = {
      worker_id: "worker/one", tab_id: null, intent: "fixture", spawned_by: "test", spawned_at: 0,
      status: "done", progress_text: null, touched_nodes: [], color: "", report: null, done_at: 1,
    };
    request.mockResolvedValue(new Response("opaque log output"));
    await render(<AgentRunLogPopup worker={worker} api={defaultConfig().api} onClose={() => {}} />);
    expect(request).toHaveBeenCalledWith("/api/mem/workers/worker%2Fone/log", {}, true);
    expect(document.body.textContent).toContain("opaque log output");
  });

  it("preserves log query parameters, deletion and refresh after deletion", async () => {
    vi.stubGlobal("confirm", vi.fn(() => true));
    request.mockImplementation(async (_path, init) => init?.method === "DELETE"
      ? new Response(null, { status: 204 })
      : Response.json([{ id: 7, ts: 1, level: "info", source: "fixture", message: "opaque log row" }]));
    await render(<LogsTab config={defaultConfig()} />);
    const path = request.mock.calls[0][0];
    expect(path).toMatch(/^\/api\/logs\?limit=500&since=\d+$/);
    expect(host.textContent).toContain("opaque log row");
    const clear = [...host.querySelectorAll("button")].find(button => button.title.includes("7 days"));
    expect(clear).toBeDefined();
    await act(async () => clear!.click());
    expect(request.mock.calls.some(([path, init]) => /^\/api\/logs\?before=\d+$/.test(path) && init?.method === "DELETE")).toBe(true);
    expect(request).toHaveBeenCalledTimes(3);
  });

  it("renders a log HTTP failure through the existing error path", async () => {
    request.mockResolvedValue(new Response("unavailable", { status: 502 }));
    await render(<LogsTab config={defaultConfig()} />);
    expect(host.textContent).toContain("502");
  });

  it("keeps secret reads local and encodes the selected organization", async () => {
    const config = defaultConfig();
    config.integrations.secrets.default_organization = "org/a b";
    request.mockImplementation(async path => {
      if (path.endsWith("/status")) return Response.json({ backend: "local", status: { status: "unlocked" } });
      if (path.includes("/items")) return Response.json({ items: [] });
      if (path.includes("/organizations")) return Response.json({ organizations: [] });
      return Response.json({ collections: [] });
    });
    await render(<SecretsTab config={config} onUpdate={() => {}} />);
    expect(request.mock.calls.map(([path]) => path)).toEqual([
      "/api/secrets/status", "/api/secrets/items?limit=10", "/api/secrets/organizations",
      "/api/secrets/collections?organization=org%2Fa%20b",
    ]);
  });

  it("keeps the secret backend mutation body and structured refusal", async () => {
    request.mockImplementation(async (_path, init) => init?.method === "POST"
      ? Response.json({ error: { message: "opaque backend refusal" } }, { status: 409 })
      : Response.json({ backend: "local", status: { status: "locked" } }));
    await render(<SecretsTab config={defaultConfig()} onUpdate={() => {}} />);
    const backend = host.querySelector<HTMLSelectElement>("select")!;
    await act(async () => {
      backend.value = "mock";
      backend.dispatchEvent(new Event("change", { bubbles: true }));
    });
    const mutation = request.mock.calls.find(([, init]) => init?.method === "POST");
    expect(mutation?.[0]).toBe("/api/secrets/backend");
    expect(JSON.parse(String(mutation?.[1]?.body))).toEqual({ backend: "mock" });
    expect(host.textContent).toContain("opaque backend refusal");
  });

  it("keeps activity fallback polling and advances its cursor from delivered events", async () => {
    vi.useFakeTimers();
    vi.mocked(listen).mockRejectedValue(new Error("event port unavailable"));
    vi.stubGlobal("requestAnimationFrame", vi.fn(() => 1));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    const event = { op: "read", mem_ids: ["memory-one"], source: { kind: "cli", tab_id: null, worker_id: null, agent_provider: null }, ts_ms: Date.now() + 10_000 };
    request.mockResolvedValue(Response.json({ events: [event] }));
    let result!: UseActivityPulsesResult;
    function Probe() { result = useActivityPulses(defaultConfig().api); return null; }
    await render(<Probe />);
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(result.recentEvents).toEqual([event]);
    request.mockResolvedValue(Response.json({ events: [] }));
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(request.mock.calls[1][0]).toBe(`/api/mem/activity/recent?since_ms=${event.ts_ms}`);
    await act(async () => root.unmount());
    const count = request.mock.calls.length;
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(request).toHaveBeenCalledTimes(count);
  });

  it("rebinds once without loading unrelated config and preserves HTTP error text", async () => {
    request.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await rebindTabSession("tab/one", "session-two");
    expect(request.mock.calls[0][0]).toBe("/api/tabs/tab%2Fone/session/rebind");
    expect(request.mock.calls[0][1]?.method).toBe("POST");
    expect(JSON.parse(String(request.mock.calls[0][1]?.body))).toEqual({ session_id: "session-two" });
    request.mockResolvedValueOnce(new Response("opaque rebind refusal", { status: 409 }));
    await expect(rebindTabSession("tab/one", "session-two")).rejects.toThrow("opaque rebind refusal");
    expect(request).toHaveBeenCalledTimes(2);
  });
});
