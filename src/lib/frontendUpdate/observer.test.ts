import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const script = readFileSync("src-tauri/src/frontend_bundle/observer.js", "utf8");
const ticket = { releaseId: "frontend-v1.0.0", generation: 1, token: "health" };
const context = { apiOrigin: "http://localhost:7686", token: "owner", clientId: "client-a", ticket, attempted: [] as string[], bundled: { index: "/bundled/index.html", minApiVersion: 2, maxApiVersion: 2 } };

type Listener = (event: { detail?: unknown }) => void;

function harness(responses: unknown[], boot = context) {
  const listeners = new Map<string, Listener>();
  const calls: { path: string; body: unknown; authorization: string }[] = [];
  const stored: string[] = [];
  let deadline!: () => void;
  let navigate!: (path: string) => void;
  const navigated = new Promise<string>(resolve => { navigate = resolve; });
  const fetcher = async (path: string, init: { body: string; headers: { Authorization: string } }) => {
    calls.push({ path, body: JSON.parse(init.body), authorization: init.headers.Authorization });
    const response = responses.shift();
    return { ok: response !== undefined, json: async () => response };
  };
  new Function("sessionStorage", "location", "addEventListener", "fetch", "AbortSignal", script)(
    { getItem: () => JSON.stringify(boot), setItem: (_key: string, value: string) => stored.push(value) },
    { origin: boot.apiOrigin, pathname: `/frontend/${ticket.releaseId}/index.html`, replace: navigate },
    (name: string, listener: Listener) => listeners.set(name, listener), fetcher,
    { timeout: () => ({ addEventListener: (_name: string, listener: () => void) => { deadline = listener; } }) },
  );
  return { listeners, calls, stored, deadline: () => deadline(), navigated };
}

describe("stable frontend observer", () => {
  it("detects missing render independently of candidate JS and navigates previous compatible release", async () => {
    const h = harness([
      { apiVersion: 2, active: { releaseId: "previous", minApiVersion: 2, maxApiVersion: 2 } },
      { releaseId: "previous", generation: 1, claims: { "client-a": "previous-health" } },
    ]);
    expect(h.calls).toEqual([]);
    h.deadline();
    expect(await h.navigated).toBe("/frontend/previous/index.html?generation=1");
    expect(h.calls.map(call => call.path)).toEqual([
      "http://localhost:7686/api/frontend/rollback", "http://localhost:7686/api/frontend/activate",
    ]);
    expect(h.calls[0].body).toEqual({ releaseId: ticket.releaseId, generation: 1 });
    expect(JSON.parse(h.stored[0]).attempted).toEqual([ticket.releaseId]);
  });
  it("ignores foreign renders and only exact root evidence acknowledges health", async () => {
    const h = harness([{}]);
    h.listeners.get("codeterm:frontend-root-rendered")!({ detail: { ...ticket, generation: 0 } });
    expect(h.calls).toEqual([]);
    h.listeners.get("codeterm:frontend-root-rendered")!({ detail: ticket });
    expect(h.calls).toHaveLength(1);
    expect(h.calls[0].path).toContain("/healthy");
    expect(h.calls[0].authorization).toBe("Bearer owner");
    h.deadline();
    expect(h.calls).toHaveLength(1);
  });
  it("falls back silently to a compatible bundled shell after boot exception", async () => {
    const h = harness([{ apiVersion: 2, active: null }]);
    h.listeners.get("error")!({});
    h.listeners.get("unhandledrejection")!({});
    expect(await h.navigated).toBe("http://localhost:7686/bundled/index.html");
    expect(h.calls).toHaveLength(1);
    expect(JSON.parse(h.stored[0]).ticket).toBeNull();
  });
  it("bounds recovery attempts without a reload loop", () => {
    const h = harness([], { ...context, attempted: ["old-1", "old-2"] });
    h.deadline();
    expect(h.calls).toEqual([]);
    expect(h.stored).toEqual([]);
  });
});
