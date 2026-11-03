import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { RunChatPopup } from "./RunChatPopup";
import { clearTranscriptWindows } from "../Terminal/transcriptStore";
import type { ChatMsgEvent } from "../Terminal/chatSessionAuthority";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const tabState = vi.hoisted(() => ({ tab: { agent_session_id: "session-1", session_binding_nonce: "nonce" } as { agent_session_id: string; session_binding_nonce: string } | undefined }));
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: () => tabState.tab }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  __setInvokeForTest(null);
  __setListenForTest(null);
  clearTranscriptWindows();
  vi.restoreAllMocks();
  vi.useRealTimers();
  tabState.tab = { agent_session_id: "session-1", session_binding_nonce: "nonce" };
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
