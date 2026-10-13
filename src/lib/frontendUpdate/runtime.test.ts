import { describe, expect, it } from "vitest";
import { initializeFrontendUpdateRuntime, ROOT_RENDERED_EVENT } from "./runtime";
import { createFrontendSignals } from "./signals";
import { TransportError } from "../transportClient";
import { frontendUpdateDiagnostic } from "./bootstrapModel";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../terminalViewerEvents";
import type { StreamEnd, TransportClient } from "../transportClient";
import type { CheckpointWrite, ClientCheckpoint } from "./checkpoint";

const checkpoint: ClientCheckpoint = { activeWorkspaceId: "w", activeTabId: "t", drafts: { t: { text: "last edit", attachmentIds: ["a"], replyTo: null } }, scroll: { t: { chat: "anchor", terminal: "42" } } };
const safety = { focused: false, idle: true, typing: false, composing: false, modalOpen: false, pendingUploads: 0 };
const release = { releaseId: "frontend-v1.0.0", version: "1.0.0", minApiVersion: 2, maxApiVersion: 2 };

function fixture(patch = {}, payload = checkpoint) {
  let receive!: (name: string, data: unknown) => void;
  const calls: { method: string; path: string; body?: unknown }[] = [];
  const events: Event[] = [];
  const eventTarget = new EventTarget();
  eventTarget.addEventListener(ROOT_RENDERED_EVENT, event => events.push(event));
  let end!: (end: StreamEnd) => void;
  let subscriptions = 0;
  let disposals = 0;
  const stored = new Map<string, string>();
  const navigated: string[] = [];
  const restored: ClientCheckpoint[] = [];
  const signals = createFrontendSignals({ ...checkpoint, drafts: {} }, { ...safety, ...patch }, value => { restored.push(value); });
  const client = {
    localJson: async <T>(method: string, path: string, body?: unknown): Promise<T> => {
      calls.push({ method, path, body });
      if (path.endsWith("checkpoint")) return (method === "GET" ? { revision: 9, payload } : body) as T;
      if (path.endsWith("activate")) return { ...release, generation: 7, claims: { clientA: "health-secret" } } as T;
      return { apiVersion: 2, pending: null, quarantined: [] } as T;
    },
    origin: () => "https://daemon.example", currentToken: () => "api-secret",
    subscribe: (path: string, listener: typeof receive, options: { onEnd: typeof end }) => { expect(path).toBe("/api/events/stream"); receive = listener; end = options.onEnd; subscriptions++; return () => { disposals++; }; },
  } as TransportClient;
  const options = { clientId: "clientA", signals, storage: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => { stored.set(key, value); } },
    navigate: (path: string) => { expect(stored.has("codeterm.frontend.boot")).toBe(true); navigated.push(path); },
    events: eventTarget,
    event: (detail: { releaseId: string; generation: number }) => new CustomEvent(ROOT_RENDERED_EVENT, { detail }),
    boot: { apiOrigin: "https://daemon.example", token: "api-secret", clientId: "clientA", ticket: { releaseId: release.releaseId, generation: 4, token: "old-health" }, attempted: [], bundled: { index: "/bundled/index.html", minApiVersion: 2, maxApiVersion: 2 } },
    bundled: { index: "/bundled/index.html", minApiVersion: 2, maxApiVersion: 2 }, onError: (error: unknown) => { throw error; },
  };
  return { client, options, signals, calls, events, navigated, stored, restored, end: (reason: StreamEnd["reason"]) => end({reason}), subscriptions: () => subscriptions, disposals: () => disposals, receive: (name: string, data: unknown) => receive(name, data) };
}

describe("frontend runtime integration", () => {
  it("reconnects a clean generation end, waits for recovery after failure, and stops on disposal", async () => {
    const f = fixture();
    const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
    f.end("eof");
    await Promise.resolve();
    expect(f.subscriptions()).toBe(2);
    expect(runtime.snapshot().revision).toBe(9);
    f.end("http");
    await Promise.resolve();
    expect(f.subscriptions()).toBe(2);
    f.options.events.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
    expect(f.subscriptions()).toBe(3);
    runtime.dispose();
    f.end("eof");
    f.options.events.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
    await Promise.resolve();
    expect(f.subscriptions()).toBe(3);
    expect(f.disposals()).toBe(1);
  });
  it("hydrates before returning the render boundary and reports only the exact committed root identity", async () => {
    const f = fixture();
    const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
    expect(f.restored).toEqual([checkpoint]);
    expect(runtime.snapshot().revision).toBe(9);
    expect(f.events).toEqual([]);
    runtime.rootRendered(); runtime.rootRendered();
    expect(f.events).toHaveLength(1);
    expect(f.events[0].type).toBe(ROOT_RENDERED_EVENT);
    expect((f.events[0] as CustomEvent).detail).toEqual({ releaseId: release.releaseId, generation: 4 });
    runtime.dispose();
  });
  for (const patch of [{ focused: true }, { pendingUploads: 1 }]) {
    it(`defers for ${Object.keys(patch)[0]} and persists the last edit before navigation`, async () => {
      const f = fixture(patch);
      const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
      f.receive("frontend-update-ready", release);
      expect(f.calls.filter(call => call.method === "PUT")).toEqual([]);
      f.signals.edit({ ...checkpoint, activeTabId: "next" });
      f.signals.signal(safety);
      await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
      const write = f.calls.find(call => call.method === "PUT")?.body as CheckpointWrite;
      expect(write.revision).toBe(10);
      expect(write.payload.activeTabId).toBe("next");
      expect(f.navigated).toEqual(["/frontend/frontend-v1.0.0/index.html?generation=7"]);
      expect(f.navigated[0]).not.toContain("secret");
      runtime.dispose();
    });
  }
  it("cancels a quarantined pending release delivered by status SSE", async () => {
    const f = fixture({ modalOpen: true });
    const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
    f.receive("frontend-update-ready", release);
    f.receive("frontend-status", { apiVersion: 2, pending: null, quarantined: [release.releaseId] });
    f.signals.signal(safety);
    expect(runtime.snapshot().pending).toBeNull();
    expect(f.navigated).toEqual([]);
    runtime.dispose();
  });
});

it("aggregates concurrent uploads and captures draft metadata and scroll without a render delay", async () => {
  const { frontendSignals, editFrontendDraft, editFrontendScroll, signalFrontendUploads } = await import("./appState");
  const first = {}; const second = {};
  signalFrontendUploads(first, 1); signalFrontendUploads(second, 2);
  expect(frontendSignals.safety().pendingUploads).toBe(3);
  signalFrontendUploads(first, 0);
  expect(frontendSignals.safety().pendingUploads).toBe(2);
  editFrontendDraft("tab-session-chat-capsules:t", '[{"label":"image1","path":"/uploads/file.png","kind":"image"}]');
  editFrontendScroll("t", "chat", '{"anchor":"row"}');
  expect(frontendSignals.checkpoint().drafts["tab-session-chat:t"].attachmentIds).toEqual(["image1"]);
  expect(frontendSignals.checkpoint().drafts["tab-session-chat:t"].capsules?.image1.path).toBe("/uploads/file.png");
  expect(frontendSignals.checkpoint().scroll.t.chat).toBe('{"anchor":"row"}');
  signalFrontendUploads(second, 0);
});


it("reports checkpoint and status failure reasons before subscribing to swaps", async () => {
  for (const failedPath of ["checkpoint", "status"]) {
    const f = fixture();
    const json = f.client.localJson;
    f.client.localJson = async (method, path, body, options) => {
      if (path.endsWith(failedPath)) throw new TransportError(404, path, "opaque-response");
      return json(method, path, body, options);
    };
    let failure: unknown;
    try { await initializeFrontendUpdateRuntime(f.client, f.options); } catch (error) { failure = error; }
    expect(frontendUpdateDiagnostic(failure)).toMatchObject({ reason: `${failedPath}-unavailable`, httpStatus: 404 });
    expect(f.subscriptions()).toBe(0);
  }
});

it("registers the browser draft restorer before daemon hydration and collects local fallback scopes", async () => {
  const { initializeBrowserFrontendUpdates } = await import("./browser");
  const { frontendSignals } = await import("./appState");
  const { COMPOSER_DRAFT_PREFIX } = await import("../composerDraftGc");
  const f = fixture({}, { ...checkpoint, scroll: {} });
  localStorage.clear();
  localStorage.setItem(COMPOSER_DRAFT_PREFIX + "t", "stale browser draft");
  localStorage.setItem(COMPOSER_DRAFT_PREFIX + "tab-session-chat:other", "fallback draft");
  const browser = await initializeBrowserFrontendUpdates(f.client, f.options);
  try {
    expect(frontendSignals.checkpoint().drafts.t).toEqual(checkpoint.drafts.t);
    expect(localStorage.getItem(COMPOSER_DRAFT_PREFIX + "t")).toBe("last edit");
    expect(frontendSignals.checkpoint().drafts["tab-session-chat:other"].text).toBe("fallback draft");
  } finally {
    browser.dispose();
    localStorage.clear();
  }
});
