import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { RunChatPopup } from "./RunChatPopup";
import { clearTranscriptWindows } from "../Terminal/transcriptStore";
import type { ChatMsgEvent } from "../Terminal/chatSessionAuthority";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { CronJob, CronRunInfo } from "../../lib/types";
import { CronDrawerSection } from "../MeshPanel/CronDrawerSection";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import type { Service } from "../MeshPanel/types";

const tabState = vi.hoisted(() => ({ tab: { agent_session_id: "session-1", session_binding_nonce: "nonce" } as { agent_session_id: string; session_binding_nonce: string } | undefined }));
const transcriptRelay = vi.hoisted(() => ({ calls: [] as Array<{ peerId?: string; method: string; path: string }> }));
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: () => tabState.tab }));
vi.mock("../../lib/daemonBootstrap", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/daemonBootstrap")>(),
  daemonTransportForPeer: (peerId?: string) => ({
    json: async (method: string, path: string) => {
      transcriptRelay.calls.push({ peerId, method, path });
      return { messages: [] };
    },
  }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  __setInvokeForTest(null);
  __setListenForTest(null);
  clearTranscriptWindows();
  vi.restoreAllMocks();
  vi.useRealTimers();
  tabState.tab = { agent_session_id: "session-1", session_binding_nonce: "nonce" };
  transcriptRelay.calls = [];
});

const drawerRun: CronRunInfo = {
  run_id: "drawer-run",
  started_at: 1,
  finished_at: 2,
  status: "Success",
  tab_id: null,
  session_id: "drawer-session",
  cwd: "/workspace",
  provider: "claude",
  owner_instance_id: null,
};

const drawerJob: CronJob = {
  id: "drawer-job",
  name: "Drawer job",
  description: null,
  workspace_id: "workspace-1",
  schedule: "0 * * * *",
  enabled: true,
  agent: "claude",
  model: null,
  reasoning_effort: null,
  task: "task",
  cwd: null,
  role: null,
  oneshot: false,
  max_history: 10,
  created_at: 1,
  last_run: drawerRun,
  next_run: null,
  jitter_max_ms: null,
};

function drawerService(id: string, sourcePeerId?: string): Service {
  return {
    id,
    name: "Drawer job",
    kind: "cron",
    health_mode: "http",
    health_interval: 60,
    health: { status: "healthy" },
    push_enabled: false,
    registered_at: 1,
    updated_at: 1,
    enabled: true,
    machine_id: sourcePeerId ?? "local-machine",
    machine_name: "Machine",
    schedule: "0 * * * *",
    capabilities: { get: { allowed: true }, history: { allowed: true } },
    source_peer_id: sourcePeerId,
  } as Service;
}

const drawerActions: MeshResourceActionAdapter = {
  get: async () => drawerJob,
  history: async () => [drawerRun],
  trigger: async () => undefined,
  setEnabled: async () => undefined,
  cancel: async () => undefined,
  updateSchedule: async () => undefined,
  remove: async () => undefined,
  regenerateToken: async () => "",
};

async function openDrawerTranscript(service: Service) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => {
      root.render(<CronDrawerSection service={service} machineName="Local machine" onUpdate={() => undefined} actionAdapter={drawerActions} />);
      await Promise.resolve();
    });
    const open = host.querySelector<HTMLButtonElement>('button[aria-label^="View transcript:"]');
    expect(open).not.toBeNull();
    await act(async () => open?.click());
    await act(async () => { await Promise.resolve(); });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
}

it("normalizes the cron id in local Jobs drawer transcript requests", async () => {
  await openDrawerTranscript(drawerService("cron_local-job"));
  expect(transcriptRelay.calls).toEqual([{
    peerId: undefined,
    method: "GET",
    path: "/api/crons/local-job/history/drawer-run",
  }]);
});

it("normalizes the cron id and routes remote Mesh drawer transcripts to their peer", async () => {
  await openDrawerTranscript(drawerService("cron_remote-job", "machine-air"));
  expect(transcriptRelay.calls).toEqual([{
    peerId: "machine-air",
    method: "GET",
    path: "/api/crons/remote-job/history/drawer-run",
  }]);
});

it("reads a completed run by its retained session without requiring its closed tab", async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const answer: NormalizedChatMessage = { id: "retained-answer", type: "assistant", content: "Opaque retained round answer" };
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return { messages: [answer] } as T;
  });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RunChatPopup title="Finished fixture" status="Failed" provider="" cwd="/fixture" sessionId="retained-session" onClose={() => undefined} />));
    expect(calls).toEqual([{ command: "session_transcript", args: { provider: "", cwd: "/fixture", sessionId: "retained-session" } }]);
    expect(document.body.textContent).toContain("Opaque retained round answer");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("loads a cron transcript from the run owner using cron and run identity", async () => {
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<RunChatPopup
      title="Remote cron run"
      status="Success"
      provider="codex"
      cwd="/old"
      sessionId="session-owned-on-peer"
      cronId="cron-1"
      runId="run-1"
      ownerInstanceId="machine-air"
      onClose={() => undefined}
    />));
    expect(transcriptRelay.calls).toEqual([{
      peerId: "machine-air",
      method: "GET",
      path: "/api/crons/cron-1/history/run-1",
    }]);
  } finally {
    await act(async () => root.unmount());
  }
});

it("renders live transcript events through the authoritative subscription without polling", async () => {
  const commands: string[] = [];
  let handler: ((event: { payload: ChatMsgEvent }) => void) | undefined;
  const authority = { source_session_id: "session-1", session_binding_nonce: "nonce" };
  const row: NormalizedChatMessage = { id: "live-answer", type: "assistant", content: "First answer", seq: 0 };
  const frames: FrameRequestCallback[] = [];
  vi.spyOn(globalThis, "requestAnimationFrame").mockImplementation((callback) => { frames.push(callback); return frames.length; });
  const intervals = vi.spyOn(window, "setInterval");
  __setListenForTest(async (name, callback) => {
    if (name === "tab-chat-msg-run-tab") handler = callback as typeof handler;
    return () => { handler = undefined; };
  });
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    commands.push(command);
    if (command === "session_transcript") return { messages: [{ ...row, content: "Final retained answer" }] } as T;
    if (command === "chat_send_status") return [] as T;
    if (command === "chat_start") return { ...authority, text: "", start_offset: 0, offset: 1,
      offset_kind: "items", pending: false, messages: [row], subscription_id: args?.subscriptionId } as T;
    return {} as T;
  });
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<RunChatPopup title="Live run" status="Running" provider="fixture" cwd="/workspace" sessionId="session-1" tabId="run-tab" live onClose={() => undefined} />));
    expect(document.body.textContent).toContain("First answer");
    expect(handler).toBeDefined();
    await act(async () => {
      handler?.({ payload: { ...authority, tab_id: "run-tab", messages: [{ ...row, content: "Event delivered answer" }] } });
      frames.splice(0).forEach(callback => callback(0));
    });
    expect(document.body.textContent).toContain("Event delivered answer");
    expect(document.body.textContent).not.toContain("First answer");
    expect(commands.filter(command => command === "chat_start")).toHaveLength(1);
    expect(commands).not.toContain("session_transcript");
    expect(intervals).not.toHaveBeenCalled();
    tabState.tab = undefined;
    await act(async () => root.render(<RunChatPopup title="Live run" status="Running" provider="fixture" cwd="/workspace" sessionId="session-1" tabId="run-tab" live onClose={() => undefined} />));
    expect(document.body.textContent).toContain("Final retained answer");
    expect(commands.filter(command => command === "session_transcript")).toHaveLength(1);
  } finally {
    await act(async () => root.unmount());
  }
  expect(commands).toContain("chat_stop");
});
