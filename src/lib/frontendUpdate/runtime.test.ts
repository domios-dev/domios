import { describe, expect, it, vi } from "vitest";
import { initializeFrontendUpdateRuntime, ROOT_RENDERED_EVENT } from "./runtime";
import { createFrontendSignals } from "./signals";
import { TransportError } from "../transportClient";
import { frontendUpdateDiagnostic } from "./bootstrapModel";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../terminalViewerEvents";
import type { StreamEnd, TransportClient } from "../transportClient";
import type { CheckpointWrite, ClientCheckpoint } from "./checkpoint";
import { frontendApiContract } from "../frontendApiContract";

const API = frontendApiContract.maxApiVersion;

const checkpoint: ClientCheckpoint = { activeWorkspaceId: "w", activeTabId: "t", drafts: { t: { text: "last edit", attachmentIds: ["a"], replyTo: null } }, scroll: { t: { chat: "anchor", terminal: "42" } } };
const safety = { focused: false, idle: true, typing: false, composing: false, modalOpen: false, pendingUploads: 0 };
const release = { releaseId: "frontend-v1.0.0", version: "1.0.0", minApiVersion: API, maxApiVersion: API };

function fixture(patch = {}, payload = checkpoint, pending: typeof release | null = null) {
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
      return { apiVersion: API, pending, quarantined: [] } as T;
    },
    origin: () => "https://daemon.example", currentToken: () => "api-secret",
    subscribe: (path: string, listener: typeof receive, options: { onEnd: typeof end }) => { expect(path).toBe("/api/events/stream"); receive = listener; end = options.onEnd; subscriptions++; return () => { disposals++; }; },
  } as TransportClient;
  const options = { clientId: "clientA", signals, storage: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => { stored.set(key, value); } },
    navigate: (path: string) => { expect(stored.has("codeterm.frontend.boot")).toBe(true); navigated.push(path); },
    events: eventTarget,
    event: (detail: { releaseId: string; generation: number }) => new CustomEvent(ROOT_RENDERED_EVENT, { detail }),
    boot: { apiOrigin: "https://daemon.example", token: "api-secret", clientId: "clientA", ticket: { releaseId: release.releaseId, generation: 4, token: "old-health" }, attempted: [], bundled: { index: "/bundled/index.html", minApiVersion: API, maxApiVersion: API } },
    bundled: { index: "/bundled/index.html", minApiVersion: API, maxApiVersion: API }, onError: (error: unknown) => { throw error; },
  };
  return { client, options, signals, calls, events, navigated, stored, restored, end: (reason: StreamEnd["reason"]) => end({reason}), subscriptions: () => subscriptions, disposals: () => disposals, receive: (name: string, data: unknown) => receive(name, data) };
}

it("updates one draft and scroll branch without cloning unrelated checkpoint data", () => {
  const drafts: ClientCheckpoint["drafts"] = {
    target: { text: "before", attachmentIds: [], replyTo: null },
  };
  const scroll: ClientCheckpoint["scroll"] = {};
  for (let index = 0; index < 64; index++) {
    const key = `retained-${index}`;
    drafts[key] = {
      text: "x".repeat(8_000), attachmentIds: [`attachment-${index}`], replyTo: null,
      capsules: Object.fromEntries(Array.from({ length: 8 }, (_, capsule) => {
        const label = `image-${capsule}`;
        return [label, { label, kind: "image" as const, path: `/uploads/${index}/${capsule}.png` }];
      })),
    };
    scroll[key] = { terminal: `terminal-${index}`, chat: `chat-${index}` };
  }
  const signals = createFrontendSignals({ activeWorkspaceId: "w", activeTabId: "target", drafts, scroll }, safety, () => {});
  const clone = vi.spyOn(globalThis, "structuredClone");
  let publications = 0;
  const unsubscribe = signals.subscribe(change => { if (change.type === "checkpoint") publications += 1; });
  try {
    const before = signals.checkpointView();
    const retainedDraft = before.drafts["retained-0"];
    const retainedScroll = before.scroll["retained-0"];
    clone.mockClear();
    signals.editDraft("target", "after");
    signals.editScroll("target", "chat", "new-anchor");
    expect(clone).not.toHaveBeenCalled();
    expect(publications).toBe(2);
    const changed = signals.checkpointView();
    expect(changed.drafts.target.text).toBe("after");
    expect(changed.scroll.target.chat).toBe("new-anchor");
    expect(changed.drafts["retained-0"]).toBe(retainedDraft);
    expect(changed.scroll["retained-0"]).toBe(retainedScroll);
    signals.editDraft("target", "after");
    signals.editScroll("target", "chat", "new-anchor");
    expect(publications).toBe(2);
    const external = signals.checkpoint();
    external.drafts.target.text = "external mutation";
    external.scroll.target.chat = "external mutation";
    expect(signals.checkpointView().drafts.target.text).toBe("after");
    expect(signals.checkpointView().scroll.target.chat).toBe("new-anchor");
  } finally {
    unsubscribe();
    clone.mockRestore();
  }
});

describe("frontend runtime integration", () => {
  it("waits for event-stream recovery after EOF or failure and stops on disposal", async () => {
    const f = fixture();
    const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
    expect(runtime.snapshot().connection).toBe("connected");
    f.end("eof");
    await Promise.resolve();
    expect(f.subscriptions()).toBe(1);
    expect(runtime.snapshot().connection).toBe("unavailable");
    expect(runtime.snapshot().revision).toBe(9);
    f.options.events.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
    expect(runtime.snapshot().connection).toBe("reconnecting");
    expect(f.subscriptions()).toBe(2);
    f.receive("frontend-status", { apiVersion: API, pending: null, quarantined: [] });
    expect(runtime.snapshot().connection).toBe("connected");
    f.end("http");
    expect(runtime.snapshot().connection).toBe("unavailable");
    expect(f.subscriptions()).toBe(2);
    f.options.events.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
    expect(runtime.snapshot().connection).toBe("reconnecting");
    expect(f.subscriptions()).toBe(3);
    runtime.dispose();
    f.end("eof");
    f.options.events.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
    await Promise.resolve();
    expect(f.subscriptions()).toBe(3);
    expect(f.disposals()).toBe(1);
  });
  it("retains current draft, attachment and selection while the frontend connection is unavailable", async () => {
    const f = fixture();
    const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
    const latest = structuredClone(checkpoint);
    latest.activeTabId = "new-selection";
    latest.drafts.t.text = "last local edit";
    latest.drafts.t.attachmentIds.push("new-attachment");
    f.signals.edit(latest);
    f.end("http");
    expect(runtime.snapshot().connection).toBe("unavailable");
    expect(runtime.snapshot().checkpoint).toEqual(latest);
    expect(f.calls.every(call => call.method === "GET")).toBe(true);
    f.options.events.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
    expect(runtime.snapshot().connection).toBe("reconnecting");
    f.receive("frontend-status", { apiVersion: API, pending: null, quarantined: [] });
    expect(runtime.snapshot().connection).toBe("connected");
    expect(runtime.snapshot().checkpoint).toEqual(latest);
    runtime.dispose();
  });
  it("propagates local branch edits and isolates mutable runtime snapshots", async () => {
    const f = fixture();
    const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
    const initialRevision = runtime.snapshot().revision;
    f.signals.editDraft("t", "latest local edit");
    f.signals.editScroll("t", "chat", "latest anchor");
    const current = runtime.snapshot();
    expect(current.checkpoint.drafts.t.text).toBe("latest local edit");
    expect(current.checkpoint.scroll.t.chat).toBe("latest anchor");
    expect(current.revision).toBe(initialRevision + 2);
    current.checkpoint.drafts.t.text = "external mutation";
    current.checkpoint.scroll.t.chat = "external mutation";
    expect(runtime.snapshot().checkpoint.drafts.t.text).toBe("latest local edit");
    expect(runtime.snapshot().checkpoint.scroll.t.chat).toBe("latest anchor");
    runtime.dispose();
  });
  it("keeps an incompatible frontend API unavailable and ignores update offers", async () => {
    const f = fixture();
    const errors: unknown[] = [];
    f.options.onError = error => { errors.push(error); throw error; };
    const runtime = await initializeFrontendUpdateRuntime(f.client, f.options);
    expect(() => f.receive("frontend-status", { apiVersion: API + 1, pending: release, quarantined: [] })).toThrow();
    f.receive("frontend-update-ready", release);
    expect(runtime.snapshot().connection).toBe("unavailable");
    expect(runtime.snapshot().pending).toBeNull();
    expect(errors).toHaveLength(1);
    expect(f.calls.every(call => call.method === "GET")).toBe(true);
    runtime.dispose();
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
    f.receive("frontend-status", { apiVersion: API, pending: null, quarantined: [release.releaseId] });
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

it("restores a detached scroll after a fresh module boot ahead of an older daemon checkpoint", async () => {
  const key = "own-tab:reload-session";
  const reading = { mode: "detached" as const, scrollTop: 2034,
    anchor: { key: "own-message", ordinal: 0, offset: -43 } };
  sessionStorage.clear();
  const previous = await import("../../components/Terminal/chatViewport");
  previous.setPersistedChatScrollState(key, reading);
  vi.resetModules();
  const fresh = await import("../../components/Terminal/chatViewport");
  expect(fresh.getPersistedChatScrollState(key)).toEqual(reading);
  const { initializeBrowserFrontendUpdates } = await import("./browser");
  const { frontendSignals } = await import("./appState");
  const f = fixture({}, { ...checkpoint, scroll: { [key]: {
    chat: JSON.stringify({ mode: "pinned", scrollTop: 2933 }), terminal: null,
  } } });
  const browser = await initializeBrowserFrontendUpdates(f.client, f.options);
  try {
    expect(fresh.getPersistedChatScrollState(key)).toEqual(reading);
    expect(JSON.parse(frontendSignals.checkpoint().scroll[key].chat!)).toEqual(reading);
  } finally {
    browser.dispose();
    fresh.clearPersistedChatScrollState(key);
  }
});

it("applies staged consent once through checkpoint and activation while respecting Update-all hold", async () => {
  const f = fixture({ focused: true }, checkpoint, release);
  const options = { ...f.options, held: () => true };
  const runtime = await initializeFrontendUpdateRuntime(f.client, options);
  f.receive("frontend-update-ready", release);
  runtime.consent();
  expect(f.calls.filter(call => call.method === "PUT" || call.path.endsWith("activate"))).toEqual([]);
  const navigated = new Promise<void>(resolve => {
    const navigate = options.navigate;
    options.navigate = path => { navigate(path); resolve(); };
  });
  runtime.hold(false);
  runtime.consent();
  await navigated;
  expect(f.calls.filter(call => call.path.endsWith("activate"))).toHaveLength(1);
  expect((f.calls.find(call => call.method === "PUT")?.body as CheckpointWrite).payload.drafts.t).toEqual(checkpoint.drafts.t);
  runtime.dispose();
});

it("reopens a closed frontend from current local drafts without resurrecting an old swap checkpoint", async () => {
  localStorage.clear();
  sessionStorage.clear();
  vi.resetModules();
  const { initializeBrowserFrontendUpdates } = await import("./browser");
  const { frontendSignals } = await import("./appState");
  const { COMPOSER_DRAFT_PREFIX } = await import("../composerDraftGc");
  localStorage.setItem(COMPOSER_DRAFT_PREFIX + "tab-session-chat:t", "latest unsent edit");
  localStorage.setItem(COMPOSER_DRAFT_PREFIX + "tab-session-chat-capsules:t", '[{"label":"image","kind":"image","path":"/uploads/image.png"}]');
  const f = fixture({}, { ...checkpoint, drafts: {
    "tab-session-chat:t": { text: "old unsent edit", attachmentIds: [], replyTo: null },
    "tab-session-chat:cleared": { text: "already sent", attachmentIds: [], replyTo: null },
  } });
  const browser = await initializeBrowserFrontendUpdates(f.client, { ...f.options, boot: null });
  try {
    expect(frontendSignals.checkpoint().drafts["tab-session-chat:t"].text).toBe("latest unsent edit");
    expect(frontendSignals.checkpoint().drafts["tab-session-chat:t"].attachmentIds).toEqual(["image"]);
    expect(frontendSignals.checkpoint().drafts["tab-session-chat:cleared"]).toBeUndefined();
    expect(f.calls.filter(call => call.method !== "GET")).toEqual([]);
  } finally {
    browser.dispose();
    localStorage.clear();
  }
});
