import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatMsg } from "@codeterm/chat-core";
import type { Tab } from "../../lib/types";
import { createLiveTranscriptApi, THOUGHTS_PAGE_SIZE, TRANSCRIPT_PAGE_SIZE, type LiveTranscriptApi, type RoutedRecord, type TranscriptOverview } from "../../lib/transport/liveTranscript";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { LiveTranscriptHost } from "./LiveTranscriptPanel";
import { LIVE_TRANSCRIPT_FILES, useLiveTranscriptHost, type LiveTranscriptFiles } from "./useLiveTranscriptPanel";
import type { useLiveTranscriptCapture } from "./voice/useLiveTranscriptCapture";

const connection = vi.hoisted(() => ({ id: 1, relayed: false }));
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
  const status: TranscriptOverview["status"] = {
    committedSeq: 1000,
    trigger: "scout_then_concierge",
    scout: "unavailable",
    concierge: "ready",
    binding: { ownerTabId: shell.id, revision: 2, target: { kind: "attached", epochId: "current" } },
  };
  const api = {
    readiness: vi.fn<LiveTranscriptApi["readiness"]>().mockResolvedValue({ asr: { state: "ready" }, scout: "unavailable", concierge: "ready" }),
    overview: vi.fn<LiveTranscriptApi["overview"]>().mockImplementation(async () => ({ status: { ...status }, cursor: { ackedSeq: 995, seenSeq: 995 } })),
    history: vi.fn<LiveTranscriptApi["history"]>().mockImplementation(async (from, limit) => ({
      rows: Array.from({ length: Math.min(limit, status.committedSeq - from + 1) }, (_, index) => row(from + index)),
      gaps: [],
      nextFromSeq: from + limit <= status.committedSeq ? from + limit : null,
    })),
    events: vi.fn<LiveTranscriptApi["events"]>().mockResolvedValue({ actions: [receipt("archived", 1), receipt("current", 2)], nextAfterSeq: null }),
    epoch: vi.fn<LiveTranscriptApi["epoch"]>().mockResolvedValue({ rows: [], nextOffset: null }),
    attach: vi.fn<LiveTranscriptApi["attach"]>().mockImplementation(async binding => { status.trigger = binding.trigger; }),
    save: vi.fn<LiveTranscriptApi["save"]>().mockResolvedValue({ id: "saved" }),
    route: vi.fn<LiveTranscriptApi["route"]>().mockResolvedValue({ accepted: true }),
  };
  vi.mocked(createLiveTranscriptApi).mockReturnValue(api);
  return api;
}

let fixture: { root: Root; host: HTMLDivElement } | undefined;

async function mount() {
  const api = makeApi();
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
    api,
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

async function choose(label: string, value: string) {
  const select = Array.from(document.querySelectorAll("select")).find(el =>
    el.getAttribute("aria-label") === label || el.labels?.[0]?.firstChild?.textContent === label);
  if (!select) throw new Error(`Missing select: ${label}`);
  await act(async () => {
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
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
    const { api } = await mount();
    await click("Open hidden Transcript");
    expect(createLiveTranscriptApi).not.toHaveBeenCalledWith(hidden.id);
    await click("Open active Transcript");
    expect(api.history).toHaveBeenLastCalledWith(1001 - TRANSCRIPT_PAGE_SIZE, TRANSCRIPT_PAGE_SIZE, expect.any(AbortSignal));
    expect(panel()?.querySelectorAll('input[type="checkbox"]')).toHaveLength(TRANSCRIPT_PAGE_SIZE);
    expect(panel()?.textContent).toContain("Acknowledged through 995");
    expect(api.epoch).not.toHaveBeenCalled();
    expect(api.attach).not.toHaveBeenCalled();
    await click("Choose routing mode");
    expect(panel()?.textContent).toContain("Scout is unavailable");
    const selects = Array.from(panel()?.querySelectorAll("select") ?? []);
    expect(selects[1].textContent).not.toContain(hidden.title);
    expect(control("Apply routing").disabled).toBe(true);
    await choose("Routing mode", "manual");
    await click("Apply routing");
    expect(api.attach).toHaveBeenCalledExactlyOnceWith({ trigger: "manual", modeOnly: true }, expect.any(AbortSignal));
    expect(Array.from(panel()?.querySelectorAll('input[type="checkbox"]') ?? []).every(el => !(el as HTMLInputElement).checked)).toBe(true);
    const segment = panel()?.querySelector<HTMLInputElement>('input[aria-label$="(segment 1000)"]');
    await act(async () => { segment?.click(); });
    await click("Save to Memory");
    expect(api.save).toHaveBeenCalledExactlyOnceWith("Live Transcript selection", "Committed speech 1000", expect.any(AbortSignal));
    await click("Earlier speech");
    expect(api.history).toHaveBeenLastCalledWith(1001 - 2 * TRANSCRIPT_PAGE_SIZE, TRANSCRIPT_PAGE_SIZE, expect.any(AbortSignal));
    expect(panel()?.textContent).not.toContain("Committed speech 1000");
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
  });
});
