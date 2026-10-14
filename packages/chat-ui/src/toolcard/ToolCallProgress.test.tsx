import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { materializeToolEvent, reduceToolEvents, type NormalizedChatMessage } from "@codeterm/chat-core";
import { ToolCallBubble } from "../ToolCallBubble";
import { toolCallProgress } from "./toolCallProgressModel";

(globalThis as {IS_REACT_ACT_ENVIRONMENT?: boolean}).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | undefined;
let host: HTMLDivElement | undefined;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  vi.restoreAllMocks();
});
async function render(message: NormalizedChatMessage) {
  if (!host) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  }
  await act(async () => root?.render(<ToolCallBubble message={message} expanded={false} onToggle={() => {}} />));
  return host;
}
const call: NormalizedChatMessage = {id:"exec",type:"tool_call",content:"",toolId:"exec",toolKind:"command",toolSummary:"git status",toolStatus:"running",toolStartedAtMs:1000};

describe("tool call lifecycle", () => {
  it("keeps partial output pending and a new user message cannot silently settle explicit running work", () => {
    const partial = {...call,toolResult:"partial output"};
    expect(materializeToolEvent(partial)?.status).toBe("pending");
    const items = reduceToolEvents([partial,{id:"user",type:"user",content:"continue"}]);
    expect(items[0].kind === "tool" && items[0].event.status).toBe("pending");
  });
  it("formats elapsed time without changing lifecycle at any duration", () => {
    expect(toolCallProgress(call,601000)).toBe("Running · 10m 0s");
    expect(toolCallProgress({...call,toolStatus:"completed",toolCompletedAtMs:5000},999000)).toBe("completed · 4s");
    expect(materializeToolEvent({...call,toolStatus:"abandoned"})?.status).toBe("interrupted");
  });
  it("wires a running counter through the collapsed card and stops on an abandonment event", async () => {
    let tick: (() => void) | undefined;
    vi.spyOn(Date,"now").mockReturnValue(11000);
    vi.spyOn(window,"setInterval").mockImplementation((callback) => { tick = callback as () => void; return 42; });
    const clear = vi.spyOn(window,"clearInterval");
    const view = await render(call);
    expect(view.querySelector('[role="timer"]')?.textContent).toBe("Running · 10s");
    vi.mocked(Date.now).mockReturnValue(611000);
    await act(async () => tick?.());
    expect(view.querySelector('[role="timer"]')?.textContent).toBe("Running · 10m 10s");
    expect(call.toolStatus).toBe("running");
    await render({...call,toolStatus:"abandoned",toolInterrupted:true});
    expect(view.querySelector('[role="timer"]')?.textContent).toBe("Interrupted by a new turn");
    expect(view.querySelector('[aria-label="interrupted"]')).not.toBeNull();
    expect(clear).toHaveBeenCalledWith(42);
  });
});
