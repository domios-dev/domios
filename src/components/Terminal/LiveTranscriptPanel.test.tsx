import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatMsg } from "@codeterm/chat-core";
import type { Tab } from "../../lib/types";
import { createLiveTranscriptApi, THOUGHTS_PAGE_SIZE, TRANSCRIPT_PAGE_SIZE, type LiveTranscriptApi, type RoutedRecord, type TranscriptForward, type TranscriptOverview, type TranscriptReadiness } from "../../lib/transport/liveTranscript";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { LiveTranscriptHost } from "./LiveTranscriptPanel";
import { LIVE_TRANSCRIPT_FILES, useLiveTranscriptHost, type LiveTranscriptFiles } from "./useLiveTranscriptPanel";
import type { useLiveTranscriptCapture } from "./voice/useLiveTranscriptCapture";

const connection = vi.hoisted(() => ({ id: 1, relayed: false }));
const listen = vi.hoisted(() => vi.fn<(event: string, handler: (event: { payload: { ownerTabId?: string } }) => void) => Promise<() => void>>());
vi.mock("../../lib/bridge", async importOriginal => ({
  ...await importOriginal<typeof import("../../lib/bridge")>(),
  listen,
}));
vi.mock("../../lib/webApi", async importOriginal => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  webRequestContext: () => connection.id,
}));
vi.mock("../../lib/transport/liveTranscript", async importOriginal => ({
  ...await importOriginal<typeof import("../../lib/transport/liveTranscript")>(),
  directTranscriptAvailable: () => !connection.relayed,
  createLiveTranscriptApi: vi.fn(),
}));
vi.mock("./voice/useLiveTranscriptCapture", () => ({
  useLiveTranscriptCapture: () => {
    const [phase, setPhase] = useState<ReturnType<typeof useLiveTranscriptCapture>["phase"]>("idle");
    return {
      phase, revision: 0, provisional: phase === "recording" ? "Uncommitted words" : "", error: null,
      start: async () => setPhase("recording"),
      pause: async () => setPhase("paused"),
    };
  },
}));

function tab(id: string, title: string, props: Partial<Tab> = {}): Tab {
  return { id, title, tab_type: "Terminal", pty_id: null, cwd: null, ...props };
}

const shell = tab("shell", "Shell");
const agent = tab("agent", "Agent", { agent_type: "fixture" });
const hidden = tab("hidden", "Private router", { hidden: true, agent_type: "fixture" });

function row(seq: number) {
  return { seq, atMs: seq, content: { kind: "speech" as const, text: `Committed speech ${seq}` } };
}

function receipt(epoch: string, seq: number): RoutedRecord {
  return {
    actionSeq: seq,
    binding: { ownerTabId: shell.id, revision: seq, target: { kind: "attached", epochId: epoch } },
    action: { actionId: epoch, range: { fromSeq: seq, toSeq: seq }, atMs: seq, destination: hidden.id, rationale: `Reason for ${epoch}` },
  };
}

function makeApi() {
  let invalidated: (() => void) | undefined;
  const readiness: TranscriptReadiness = {
    asr: { state: "ready" },
    concierge: "ready",
    scout: "unavailable",
    router: {
      provider: "fixture",
      model: "base",
      providers: [
        { id: "fixture", displayName: "Fixture", defaultModel: "base", models: ["base"] },
        { id: "local", displayName: "Local", defaultModel: "small", models: ["small", "large"] },
      ],
    },
  };
  const status: TranscriptOverview["status"] = {
    committedSeq: 1000,
    trigger: "scout_then_concierge",
    scout: "unavailable",
    concierge: "ready",
    binding: { ownerTabId: shell.id, revision: 2, target: { kind: "attached", epochId: "current" } },
    router: { provider: "fixture", model: "base", activity: "idle" },
  };
  const api = {
    subscribeChanges: vi.fn<LiveTranscriptApi["subscribeChanges"]>().mockImplementation(refresh => {
      invalidated = refresh;
      return () => { invalidated = undefined; };
    }),
    readiness: vi.fn<LiveTranscriptApi["readiness"]>().mockImplementation(async () => ({ ...readiness })),
    overview: vi.fn<LiveTranscriptApi["overview"]>().mockImplementation(async () => ({ status: { ...status }, cursor: { ackedSeq: 995, seenSeq: 995 } })),
    history: vi.fn<LiveTranscriptApi["history"]>().mockImplementation(async (from, limit) => ({
      rows: Array.from({ length: Math.min(limit, status.committedSeq - from + 1) }, (_, index) => row(from + index)),
      gaps: [],
      nextFromSeq: from + limit <= status.committedSeq ? from + limit : null,
    })),
    events: vi.fn<LiveTranscriptApi["events"]>().mockResolvedValue({ actions: [receipt("archived", 1), receipt("current", 2)], nextAfterSeq: null }),
    epoch: vi.fn<LiveTranscriptApi["epoch"]>().mockResolvedValue({ rows: [], nextOffset: null }),
    attach: vi.fn<LiveTranscriptApi["attach"]>().mockImplementation(async binding => {
      if (binding.trigger) status.trigger = binding.trigger;
      if (binding.defaultMicTap !== undefined) status.defaultMicTap = readiness.defaultMicTap = binding.defaultMicTap;
      if ("configure" in binding && binding.configure) status.configured = readiness.configured = true;
      if (!("modeOnly" in binding)) {
        status.concierge = "ready";
        status.binding.target = { kind: "attached", epochId: "current" };
        if (binding.provider) status.router = { provider: binding.provider, model: binding.model, activity: "idle" };
      }
    }),
    save: vi.fn<LiveTranscriptApi["save"]>().mockResolvedValue({ id: "saved" }),
    route: vi.fn<LiveTranscriptApi["route"]>().mockResolvedValue({ accepted: true }),
    feedback: vi.fn<LiveTranscriptApi["feedback"]>(),
  };
  vi.mocked(createLiveTranscriptApi).mockReturnValue(api);
  return { api, readiness, status, async emitChange() { await act(async () => { invalidated?.(); }); } };
}

let fixture: { root: Root; host: HTMLDivElement } | undefined;

async function mount() {
  const transcript = makeApi();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  fixture = { root, host };
  let state = { tabs: { shell, agent, hidden }, workspaces: [], activeTabIds: {} } as unknown as ExtendedState;
  const store = createMuxStateStore(() => state);
  function Navigation() {
    const [active, setActive] = useState(shell);
    const transcript = useLiveTranscriptHost();
    return <>
      <button onClick={() => setActive(agent)}>Switch active tab</button>
      <button onClick={() => transcript?.open(active.id)}>Open active Transcript</button>
      <button onClick={() => transcript?.open(active.id, { autostart: true })}>Autostart active Transcript</button>
      <button onClick={() => transcript?.open(hidden.id)}>Open hidden Transcript</button>
      <output aria-label="Owner lock">{transcript?.locked ? transcript.ownerId : "unlocked"}</output>
    </>;
  }
  await act(async () => { root.render(<MuxStateStoreContext.Provider value={store}>
    <LiveTranscriptHost><Navigation /></LiveTranscriptHost>
  </MuxStateStoreContext.Provider>); });
  return {
    ...transcript,
    async switchPeer() {
      await act(async () => {
        connection.id++;
        connection.relayed = true;
        state = { ...state };
        store.notify();
      });
    },
  };
}

function control(name: string): HTMLButtonElement {
  const button = Array.from(document.querySelectorAll("button")).find(el => (el.getAttribute("aria-label") ?? el.textContent?.trim()) === name);
  if (!button) throw new Error(`Missing button: ${name}`);
  return button;
}

async function click(name: string) {
  await act(async () => { control(name).click(); });
}

/** Opens a shared DropdownSelect by its accessible name ("Label: current value") and returns its trigger.
 *  happy-dom has no PointerEvent; a bubbling Event of that type still reaches React's pointerdown listener. */
async function openSelect(label: string): Promise<HTMLButtonElement> {
  const trigger = Array.from(document.querySelectorAll<HTMLButtonElement>("button[aria-haspopup]"))
    .find(el => el.getAttribute("aria-label")?.startsWith(`${label}: `));
  if (!trigger) throw new Error(`Missing select: ${label}`);
  await act(async () => { trigger.dispatchEvent(new window.Event("pointerdown", { bubbles: true, cancelable: true })); });
  return trigger;
}

async function choose(label: string, value: string) {
  const trigger = await openSelect(label);
  const option = document.querySelector<HTMLElement>(`[data-testid="${trigger.dataset.testid}-option-${value}"]`);
  if (!option) throw new Error(`Missing option ${value} for ${label}`);
  await act(async () => { option.click(); });
}

function panel() {
  return document.querySelector('aside[aria-label="Live Transcript for Shell"]');
}

beforeEach(() => {
  vi.clearAllMocks();
  connection.id = 1;
  connection.relayed = false;
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
});

afterEach(async () => {
  if (fixture) {
    const { host, root } = fixture;
    await act(async () => { root.unmount(); });
    host.remove();
    fixture = undefined;
  }
  vi.unstubAllGlobals();
});

describe("Live Transcript panel", () => {
  it("keeps the owner and microphone across navigation and collapse, and accepts attachments with that owner", async () => {
    await mount();
    await click("Open active Transcript");
    await click("Resume");
    await click("Switch active tab");
    expect(panel()?.textContent).toContain("Uncommitted words");
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe(shell.id);
    await click("Collapse Live Transcript");
    expect(document.querySelector('[aria-label="Collapsed Live Transcript"]')?.textContent).toContain("Live · Shell");
    await click("Open active Transcript");
    expect(panel()).not.toBeNull();
    const file = new File(["image"], "attachment.png", { type: "image/png" });
    const received: LiveTranscriptFiles[] = [];
    const accept = (event: Event) => {
      const detail = (event as CustomEvent<LiveTranscriptFiles>).detail;
      received.push(detail);
      if ("blob" in detail) detail.upload = Promise.resolve("/fixture/attachment.png");
    };
    window.addEventListener(LIVE_TRANSCRIPT_FILES, accept);
    try {
      const drop = new Event("drop", { bubbles: true, cancelable: true });
      Object.defineProperty(drop, "dataTransfer", { value: { types: ["Files"], files: [file], items: [], getData: () => "" } });
      await act(async () => { panel()?.dispatchEvent(drop); });
      expect(received).toHaveLength(1);
      expect(received[0]).toMatchObject({ ownerId: shell.id, blob: file, name: file.name });
      expect(panel()?.textContent).toContain("Added to the owner’s composer.");
    } finally {
      window.removeEventListener(LIVE_TRANSCRIPT_FILES, accept);
    }
    await click("Pause");
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe("unlocked");
    await click("Open active Transcript");
    expect(document.querySelector('aside[aria-label="Live Transcript for Agent"]')).not.toBeNull();
  });

  it("starts at a bounded tail, changes mode explicitly, excludes the hidden router and saves only selected speech", async () => {
    const { api, status, emitChange } = await mount();
    await click("Open hidden Transcript");
    expect(createLiveTranscriptApi).not.toHaveBeenCalledWith(hidden.id);
    await click("Open active Transcript");
    expect(api.history).toHaveBeenLastCalledWith(1001 - TRANSCRIPT_PAGE_SIZE, TRANSCRIPT_PAGE_SIZE, expect.any(AbortSignal));
    expect(panel()?.querySelectorAll('input[type="checkbox"]')).toHaveLength(2);
    expect(panel()?.textContent).not.toContain("Acknowledged through");
    expect(api.epoch).not.toHaveBeenCalled();
    expect(api.attach).not.toHaveBeenCalled();
    await click("Choose routing mode");
    expect(panel()?.textContent).toContain("Scout is unavailable");
    const routeTo = await openSelect("Route to");
    expect(document.querySelector('[role="menu"]')?.textContent).not.toContain(hidden.title);
    expect(document.querySelector(`[data-testid="${routeTo.dataset.testid}-option-${hidden.id}"]`)).toBeNull();
    await act(async () => { document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
    expect(control("Apply routing").disabled).toBe(true);
    await choose("Routing mode", "manual");
    await click("Apply routing");
    expect(api.attach).toHaveBeenCalledExactlyOnceWith({ trigger: "manual", provider: "fixture", model: "base", configure: true }, expect.any(AbortSignal));
    expect(Array.from(panel()?.querySelectorAll('input[type="checkbox"]') ?? []).every(el => !(el as HTMLInputElement).checked)).toBe(true);
    const passage = panel()?.querySelector<HTMLInputElement>('input[aria-label$="(segments 996–1000)"]');
    expect(passage).not.toBeNull();
    await act(async () => { passage?.click(); });
    const selectedText = [996, 997, 998, 999, 1000].map(seq => `Committed speech ${seq}`).join("\n\n");
    const exportUrl = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:selection");
    const download = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    vi.useFakeTimers();
    try {
      await click("Export");
      expect(exportUrl).toHaveBeenCalledTimes(1);
      const blob = exportUrl.mock.calls[0][0];
      expect(blob).toBeInstanceOf(Blob);
      expect(await (blob as Blob).text()).toBe(selectedText);
      expect(download).toHaveBeenCalledTimes(1);
    } finally {
      vi.clearAllTimers();
      vi.useRealTimers();
      exportUrl.mockRestore();
      download.mockRestore();
    }
    await click("Save to Memory");
    expect(api.save).toHaveBeenCalledExactlyOnceWith("Live Transcript selection", selectedText, expect.any(AbortSignal));
    await click("Route selection");
    expect(api.route).toHaveBeenCalledExactlyOnceWith({ fromSeq: 996, toSeq: 1000 }, expect.any(AbortSignal));
    await click("Clear selection");
    status.lastRouteError = "Routed speech was not delivered.";
    await emitChange();
    await click("Retry");
    expect(api.route).toHaveBeenCalledTimes(2);
    expect(api.route).toHaveBeenLastCalledWith({ fromSeq: 996, toSeq: 1000 }, expect.any(AbortSignal));
    await click("Earlier speech");
    expect(api.history).toHaveBeenLastCalledWith(1001 - 2 * TRANSCRIPT_PAGE_SIZE, TRANSCRIPT_PAGE_SIZE, expect.any(AbortSignal));
    expect(panel()?.textContent).not.toContain("Committed speech 1000");
  });

  it("shows a ready Local Concierge without current thoughts", async () => {
    const { status, readiness } = await mount();
    status.binding.target = { kind: "local" };
    status.router = { provider: "local", model: "qwen/qwen3-4b", activity: "idle" };
    status.trigger = "direct_batch";
    status.scout = readiness.scout = "ready";

    await click("Open active Transcript");
    expect(control("Concierge · Local / qwen/qwen3-4b · Direct batch")).toBeTruthy();
    expect(panel()?.querySelector('[aria-label="Live Transcript setup"]')).toBeNull();
    expect(panel()?.querySelector('[aria-label="Open current Concierge thoughts"]')).toBeNull();
    expect(panel()?.textContent).not.toContain("Scout is unavailable");
  });

  it("marks a forward with local rationale as a wrong send from its Details", async () => {
    const { api, status } = await mount();
    status.binding.target = { kind: "local" };
    const forward: TranscriptForward = {
      forwardId: "local-forward", epochId: null, range: { fromSeq: 993, toSeq: 994 }, atMs: 1_000,
      targetTabId: agent.id, targetTitle: agent.title, sentText: "Inspect logs", sentExcerpt: "Inspect logs",
      rationale: "Explicitly asked Main to inspect logs.",
    };
    api.overview.mockResolvedValue({ status, cursor: null, forwards: [forward] });
    let resolveFeedback!: (value: Awaited<ReturnType<LiveTranscriptApi["feedback"]>>) => void;
    api.feedback.mockReturnValue(new Promise(resolve => { resolveFeedback = resolve; }));

    await click("Open active Transcript");
    expect(panel()?.textContent).not.toContain("Mark as wrong send");
    await click("Details");
    expect(panel()?.textContent).toContain(forward.rationale);
    expect(panel()?.textContent).not.toContain("Reasoning unavailable");
    await click("Mark as wrong send");
    expect(api.feedback).toHaveBeenCalledExactlyOnceWith(forward.forwardId, "wrong_send", expect.any(AbortSignal));
    expect(control("Mark as wrong send").disabled).toBe(true);
    await act(async () => resolveFeedback({ ok: true, forward: { ...forward, feedback: "wrong_send" } }));
    expect(panel()?.textContent).toContain("● Marked wrong");
    expect(control("Undo")).toBeTruthy();

    api.feedback.mockResolvedValueOnce({ ok: true, forward: { ...forward, feedback: "good_send" } });
    await click("Undo");
    expect(api.feedback).toHaveBeenLastCalledWith(forward.forwardId, "good_send", expect.any(AbortSignal));
    expect(panel()?.textContent).not.toContain("● Marked wrong");
    expect(control("Mark as wrong send")).toBeTruthy();
  });

  it("shows a sent receipt with live target navigation and inline epoch details", async () => {
    const { api, status } = await mount();
    api.overview.mockResolvedValue({
      status,
      cursor: { ackedSeq: 995, seenSeq: 995 },
      forwards: [{
        forwardId: "sent-1", epochId: "receipt-epoch", range: { fromSeq: 993, toSeq: 994 }, atMs: 1_000,
        targetTabId: agent.id, targetTitle: "Previous title", sentText: "Exact request sent", sentExcerpt: "Exact request",
        rationale: "The request named a destination",
      }],
    });
    await click("Open active Transcript");
    const sent = panel()?.querySelector('[data-seq="993"]')?.closest("article");
    expect(sent?.querySelector('[data-seq="994"]')).not.toBeNull();
    expect(sent?.textContent).toContain("Sent to Agent");
    expect(panel()?.textContent).not.toContain("Acknowledged through");
    const focused = vi.fn();
    window.addEventListener("codeterm:focus-tab", focused, { once: true });
    try {
      await click(agent.title);
      expect(focused).toHaveBeenCalledWith(expect.objectContaining({ detail: { tabId: agent.id } }));
    } finally {
      window.removeEventListener("codeterm:focus-tab", focused);
    }
    expect(sent?.textContent).not.toContain("Exact request sent");
    await click("Details");
    expect(sent?.textContent).toContain("What was sent");
    expect(sent?.textContent).toContain("Exact request sent");
    expect(sent?.textContent).toContain("The request named a destination");
    expect(api.epoch).not.toHaveBeenCalled();
    await click("Thoughts");
    expect(sent?.querySelector('[aria-label="Thoughts inspector"]')).not.toBeNull();
    expect(api.epoch).toHaveBeenCalledExactlyOnceWith("receipt-epoch", 0, expect.any(AbortSignal));
    await click("Details");
    expect(sent?.querySelector('[aria-label="Thoughts inspector"]')).toBeNull();
  });

  it("renders stacked receipts for one passage with only one Details block open at a time", async () => {
    const { api, status } = await mount();
    api.overview.mockResolvedValue({
      status,
      cursor: { ackedSeq: 995, seenSeq: 995 },
      forwards: [
        {
          forwardId: "sent-a", epochId: "ep-1", range: { fromSeq: 993, toSeq: 994 }, atMs: 1_000,
          targetTabId: agent.id, targetTitle: "Main", sentText: "First payload", sentExcerpt: "First payload",
        },
        {
          forwardId: "sent-b", epochId: "ep-1", range: { fromSeq: 993, toSeq: 994 }, atMs: 2_000,
          targetTabId: "atlas-id", targetTitle: "Atlas", sentText: "Second payload", sentExcerpt: "Second payload",
        },
      ],
    });
    await click("Open active Transcript");
    const sent = panel()?.querySelector('[data-seq="993"]')?.closest("article");
    const buttons = () => Array.from(sent?.querySelectorAll("button") ?? []).filter(b => b.textContent?.trim() === "Details");
    expect(sent?.textContent).toContain("Sent to Agent");
    expect(sent?.textContent).toContain("Sent to Atlas");
    expect(buttons()).toHaveLength(2);
    await act(async () => { buttons()[0].click(); });
    expect(sent?.textContent).toContain("First payload");
    expect(sent?.textContent).not.toContain("Second payload");
    expect(buttons()[0].getAttribute("aria-expanded")).toBe("true");
    expect(buttons()[1].getAttribute("aria-expanded")).toBe("false");
    await act(async () => { buttons()[1].click(); });
    expect(sent?.textContent).not.toContain("First payload");
    expect(sent?.textContent).toContain("Second payload");
    expect(buttons()[0].getAttribute("aria-expanded")).toBe("false");
    expect(buttons()[1].getAttribute("aria-expanded")).toBe("true");
  });

  it("expands older not-sent remarks in place while keeping the latest run visible", async () => {
    const { api, status } = await mount();
    api.history.mockResolvedValue({
      rows: [993, 994, 995, 996, 997, 998, 999].map(seq => ({
        ...row(seq), atMs: seq * 10_000, content: { kind: "speech", text: `Speech with sequence ${seq}` },
      })),
      gaps: [], nextFromSeq: null,
    });
    api.overview.mockResolvedValue({
      status,
      cursor: { ackedSeq: 999, seenSeq: 999 },
      forwards: [{
        forwardId: "sent-2", range: { fromSeq: 996, toSeq: 996 }, atMs: 1_000,
        targetTabId: "closed", targetTitle: "Saved title", sentText: "Request", sentExcerpt: "Request",
      }],
    });
    await click("Open active Transcript");
    const disclosure = control("3 remarks not sent");
    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    expect(panel()?.textContent).not.toContain("Speech with sequence 993");
    expect(panel()?.textContent).toContain("Speech with sequence 999");
    expect(control("Saved title")).toBeDefined();
    await click("3 remarks not sent");
    expect(disclosure.getAttribute("aria-expanded")).toBe("true");
    expect(panel()?.textContent).toContain("Speech with sequence 993");
    expect(panel()?.textContent).toContain("Speech with sequence 995");
  });

  it("reads only the chosen epoch while Thoughts are open and discards closed and peer-stale results", async () => {
    const { api, switchPeer } = await mount();
    let publishArchived!: (page: { rows: ChatMsg[]; nextOffset: number | null }) => void;
    api.epoch.mockReturnValueOnce(new Promise(resolve => { publishArchived = resolve; }));
    await click("Open active Transcript");
    await click("Log");
    expect(api.epoch).not.toHaveBeenCalled();
    await choose("Thoughts for", "archived");
    await click("Thoughts");
    const archivedSignal = api.epoch.mock.calls[0][2];
    expect(api.epoch).toHaveBeenLastCalledWith("archived", 0, archivedSignal);
    expect(panel()?.querySelector('[aria-label="Thoughts inspector"]')?.textContent).toContain("Reason for archived");
    await click("Close Thoughts");
    expect(archivedSignal.aborted).toBe(true);
    await choose("Thoughts for", "current");
    api.epoch.mockResolvedValueOnce({
      rows: [
        { id: "tool", type: "tool", seq: 0, content: "", toolName: "Published tool" },
        ...Array.from({ length: THOUGHTS_PAGE_SIZE }, (_, id) => ({
          id: String(id), type: "thinking", seq: id + 1, content: `Provider record ${id}`,
        })),
      ],
      nextOffset: THOUGHTS_PAGE_SIZE,
    });
    await click("Thoughts");
    await act(async () => { publishArchived({ rows: [{ id: "old", type: "thinking", content: "Stale archive" } as ChatMsg], nextOffset: null }); });
    const inspector = panel()?.querySelector('[aria-label="Thoughts inspector"]');
    expect(inspector?.textContent).not.toContain("Stale archive");
    expect(inspector?.textContent).toContain("Published tool");
    expect(inspector?.textContent).toContain("Provider record 14");
    expect(inspector?.textContent).not.toContain("Provider record 15");
    expect(inspector?.querySelector("textarea")).toBeNull();
    await click("Next Thoughts");
    const peerSignal = api.epoch.mock.lastCall?.[2];
    expect(api.epoch).toHaveBeenLastCalledWith("current", THOUGHTS_PAGE_SIZE, peerSignal);
    expect(panel()?.textContent).toContain("Provider-published reasoning is unavailable");
    await switchPeer();
    expect(peerSignal?.aborted).toBe(true);
    expect(panel()).toBeNull();
  });

  it("autostarts capture immediately when opened with autostart", async () => {
    await mount();
    await click("Autostart active Transcript");
    expect(panel()?.textContent).toContain("Uncommitted words");
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe(shell.id);
    await click("Pause");
    await click("Switch active tab");
    await click("Open active Transcript");
    expect(document.querySelector('aside[aria-label="Live Transcript for Agent"]')).not.toBeNull();
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe("unlocked");
  });

  it("blocks capture until Concierge is connected and starts after Start Live Mode", async () => {
    const { api, readiness, status } = await mount();
    status.committedSeq = 0;
    status.binding.target = { kind: "detached" };
    status.concierge = "needs_attachment";
    status.scout = readiness.scout = "ready";
    status.configured = readiness.configured = true;

    await click("Autostart active Transcript");
    expect(panel()?.querySelector('[data-testid="live-transcript-concierge-gate"]')).not.toBeNull();
    expect(control("Start").disabled).toBe(true);
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe("unlocked");

    await click("Start Live Mode");
    expect(api.attach).toHaveBeenCalledExactlyOnceWith({ trigger: "scout_then_concierge" }, expect.any(AbortSignal));
    expect(panel()?.querySelector('[data-testid="live-transcript-concierge-gate"]')).toBeNull();
    expect(panel()?.textContent).toContain("Uncommitted words");
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe(shell.id);
  });

  it("requires explicit first-time setup and offers manual connection with Scout as the default", async () => {
    const { api, readiness, status } = await mount();
    readiness.configured = false;
    status.scout = readiness.scout = "ready";
    status.binding.target = { kind: "detached" };
    status.defaultMicTap = true;

    await click("Autostart active Transcript");
    expect(panel()?.querySelector("h2")?.textContent).toBe("Set up Live Transcript");
    expect(control("Set up with General").disabled).toBe(false);
    expect(api.attach).not.toHaveBeenCalled();
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe("unlocked");

    await click("Or connect manually");
    await choose("Concierge provider", "local");
    await choose("Concierge model", "large");
    await click("Connect Concierge & Start");
    expect(api.attach).toHaveBeenCalledExactlyOnceWith({
      trigger: "scout_then_concierge", provider: "local", model: "large", configure: true,
    }, expect.any(AbortSignal));
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe(shell.id);
  });

  it("keeps setup with General when readiness reports missing ASR and overview is unavailable", async () => {
    const { api, readiness } = await mount();
    readiness.asr = { state: "unavailable" };
    readiness.configured = false;
    api.overview.mockRejectedValue(new Error("Transcript overview is unavailable."));
    await click("Autostart active Transcript");
    expect(control("Set up with General").disabled).toBe(false);
    expect(panel()?.textContent).not.toContain("Or connect manually");
    expect(api.attach).not.toHaveBeenCalled();
  });

  it("starts directly on autostart with mic tap enabled and a configured, detached Concierge", async () => {
    const { api, readiness, status } = await mount();
    status.binding.target = { kind: "detached" };
    readiness.configured = true;
    readiness.defaultMicTap = true;
    readiness.defaultTrigger = "wake_word";
    await click("Autostart active Transcript");
    expect(api.attach).toHaveBeenCalledExactlyOnceWith({ trigger: "wake_word" }, expect.any(AbortSignal));
    expect(document.querySelector('[aria-label="Owner lock"]')?.textContent).toBe(shell.id);
  });

  it("updates Concierge activity and route errors from change events and keeps Thoughts and settings accessible", async () => {
    const { api, status, emitChange } = await mount();
    await click("Open active Transcript");
    const bar = panel()?.querySelector('[data-testid="live-transcript-concierge-bar"]');
    expect(bar?.textContent).toContain("Concierge · fixture / base · Scout then Concierge");
    expect(bar?.textContent).toContain("Watching");

    status.awaitingDelivery = true;
    await emitChange();
    expect(bar?.textContent).toContain("Reviewing…");
    status.router = { ...status.router, activity: "stuck" };
    status.lastRouteError = "The Concierge turn timed out.";
    await emitChange();
    expect(bar?.textContent).toContain("Stalled");
    expect(panel()?.textContent).toContain("Not delivered");
    expect(control("Retry").disabled).toBe(true);
    expect(panel()?.querySelector('[role="alert"]')).toBeNull();
    await click("Details");
    expect(panel()?.querySelector('[role="alert"]')?.textContent).toBe("The Concierge turn timed out.");

    await click("Concierge · fixture / base · Scout then Concierge");
    expect(panel()?.querySelector('[aria-label="Routing settings"]')).not.toBeNull();
    await click("Open current Concierge thoughts");
    expect(panel()?.querySelector('[aria-label="Routing timeline"]')).not.toBeNull();
    expect(panel()?.querySelector('[aria-label="Thoughts inspector"]')).not.toBeNull();
    expect(api.epoch).toHaveBeenCalledWith("current", 0, expect.any(AbortSignal));
  });

  it("keeps recording and Concierge status visible through an epoch rotation", async () => {
    const { status, emitChange } = await mount();
    await click("Autostart active Transcript");
    expect(panel()?.textContent).toContain("Listening");
    status.concierge = "needs_attachment";
    status.binding = { ...status.binding, target: { kind: "detached" } };
    await emitChange();
    expect(panel()?.querySelector('[aria-label="Live Transcript setup"]')).toBeNull();
    expect(panel()?.querySelector('[data-testid="live-transcript-concierge-bar"]')).not.toBeNull();
    expect(control("Pause")).toBeDefined();
  });

  it("persists the mic tap preference through the mode-only attach", async () => {
    const { api } = await mount();
    await click("Open active Transcript");
    await click("Transcript settings");
    const toggle = control("Tap mic starts Live Transcript");
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await click("Tap mic starts Live Transcript");
    expect(api.attach).toHaveBeenCalledExactlyOnceWith({
      modeOnly: true, trigger: "scout_then_concierge", defaultMicTap: true,
    }, expect.any(AbortSignal));
    expect(toggle.getAttribute("aria-checked")).toBe("true");
  });
});

it("filters bridge changes by owner and disposes a late subscription", async () => {
  const { createLiveTranscriptApi: createApi } = await vi.importActual<typeof import("../../lib/transport/liveTranscript")>("../../lib/transport/liveTranscript");
  let finishListen!: (unlisten: () => void) => void;
  const unlisten = vi.fn();
  listen.mockImplementationOnce(() => new Promise(resolve => { finishListen = resolve; }));
  const refresh = vi.fn();
  const dispose = createApi("shell").subscribeChanges(refresh);
  expect(listen).toHaveBeenCalledWith("live-transcript-changed", expect.any(Function));
  const onChange = listen.mock.lastCall![1];
  onChange({ payload: { ownerTabId: "other" } });
  expect(refresh).not.toHaveBeenCalled();
  onChange({ payload: { ownerTabId: "shell" } });
  onChange({ payload: {} });
  expect(refresh).toHaveBeenCalledTimes(2);
  dispose();
  onChange({ payload: {} });
  expect(refresh).toHaveBeenCalledTimes(2);
  finishListen(unlisten);
  await Promise.resolve();
  expect(unlisten).toHaveBeenCalledOnce();
});
