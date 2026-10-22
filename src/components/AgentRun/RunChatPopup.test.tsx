import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import { RunChatPopup } from "./RunChatPopup";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  __setInvokeForTest(null);
  vi.useRealTimers();
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

it("keeps a live transcript quiet when a polling failure self-recovers", async () => {
  vi.useFakeTimers();
  let reads = 0;
  __setInvokeForTest(async <T,>() => {
    reads++;
    if (reads === 1) throw new Error("temporary transcript read failure");
    return { messages: [] } as T;
  });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RunChatPopup
      title="Fixture run"
      status="Running"
      provider="fixture"
      cwd="/workspace"
      sessionId="session-1"
      live
      onClose={() => undefined}
    />));
    expect(host.querySelector('[role="alert"]')).toBeNull();

    await act(async () => { await vi.advanceTimersByTimeAsync(1500); });

    expect(reads).toBe(2);
    expect(host.querySelector('[role="alert"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
