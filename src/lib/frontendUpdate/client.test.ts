import { describe, expect, it } from "vitest";
import { createFrontendUpdateClient, createSwapController, releaseIndex, type FrontendHttp } from "./client";
import { initialSwapState } from "./model";
import type { CheckpointWrite, ClientCheckpoint } from "./checkpoint";

const checkpoint: ClientCheckpoint = { activeWorkspaceId: "w", activeTabId: "t", drafts: { t: { text: "unsent", attachmentIds: ["a"], replyTo: null } }, scroll: {} };
const safety = { focused: true, idle: true, typing: false, composing: false, modalOpen: false, pendingUploads: 0 };
const release = { releaseId: "frontend-v1.0.0", version: "1.0.0", minApiVersion: 2, maxApiVersion: 2 };

describe("frontend update client", () => {
  it("uses explicit checkpoint and health HTTP contracts for each client", async () => {
    const calls: unknown[][] = [];
    const http: FrontendHttp = { async json<T>(method: string, path: string, body?: unknown): Promise<T> {
      calls.push([method, path, body]);
      return (path.endsWith("activate") ? { releaseId: release.releaseId, generation: 3, claims: { clientA: "token" }, index: "unused" } : body) as T;
    } };
    const api = createFrontendUpdateClient(http, "clientA");
    const ticket = await api.activate(release.releaseId);
    await api.persist({ revision: 2, payload: checkpoint });
    await api.healthy(ticket);
    expect(calls).toEqual([
      ["POST", "/api/frontend/activate", { releaseId: release.releaseId, clients: ["clientA"] }],
      ["PUT", "/api/frontend/clients/clientA/checkpoint", { revision: 2, payload: checkpoint }],
      ["POST", "/api/frontend/healthy", { releaseId: release.releaseId, generation: 3, token: "token", clientId: "clientA" }],
    ]);
    expect(releaseIndex(ticket)).toBe("/frontend/frontend-v1.0.0/index.html?generation=3");
  });
  it("awaits explicit persistence and activation promises and reloads only once", async () => {
    let resolveWrite!: (write: CheckpointWrite) => void;
    const persisted = new Promise<CheckpointWrite>(resolve => { resolveWrite = resolve; });
    const writes: CheckpointWrite[] = [];
    const reloads: unknown[] = [];
    const api = {
      checkpoint: async () => null,
      persist: (write: CheckpointWrite) => { writes.push(write); return persisted; },
      activate: async () => ({ releaseId: release.releaseId, generation: 1, token: "token" }),
      healthy: async () => null,
      rollback: async () => null,
    };
    const controller = createSwapController(initialSwapState(2, checkpoint, safety), api, ticket => reloads.push(ticket));
    controller.dispatch({ type: "ready", release });
    expect(reloads).toEqual([]);
    resolveWrite(writes[0]);
    await persisted;
    await Promise.resolve();
    expect(reloads).toHaveLength(1);
    controller.dispatch({ type: "safety", safety });
    expect(reloads).toHaveLength(1);
  });
  it("disposes in-flight effects without reloading a detached client", async () => {
    let resolveWrite!: (write: CheckpointWrite) => void;
    let write!: CheckpointWrite;
    const persisted = new Promise<CheckpointWrite>(resolve => { resolveWrite = resolve; });
    const api = { checkpoint: async () => null, persist: (value: CheckpointWrite) => { write = value; return persisted; }, activate: async () => ({ releaseId: release.releaseId, generation: 1, token: "token" }), healthy: async () => null, rollback: async () => null };
    const controller = createSwapController(initialSwapState(2, checkpoint, safety), api, () => { throw new Error("Detached reload"); });
    controller.dispatch({ type: "ready", release });
    controller.dispose();
    resolveWrite(write);
    await persisted;
    expect(controller.snapshot().reloading).toBe(false);
  });
  it("rejects traversal in immutable URLs", () => {
    expect(() => releaseIndex({ releaseId: "../other", generation: 1, token: "x" })).toThrow();
    expect(() => createFrontendUpdateClient({ json: async <T>() => null as T }, "a/b")).toThrow();
  });
});

it("stores observer credentials before navigation without placing tokens in URLs", async () => {
  const { preparePageReload } = await import("./client");
  const events: string[] = [];
  preparePageReload({ apiOrigin: "http://localhost:7686", token: "api-secret", clientId: "clientA", ticket: { releaseId: release.releaseId, generation: 1, token: "health-secret" }, attempted: [], bundled: { index: "/bundled/index.html", minApiVersion: 2, maxApiVersion: 2 } }, { setItem: (key, value) => { expect(key).toBe("codeterm.frontend.boot"); expect(JSON.parse(value).ticket.token).toBe("health-secret"); events.push("stored"); } }, path => { expect(path).not.toContain("secret"); events.push("navigated"); });
  expect(events).toEqual(["stored", "navigated"]);
});
