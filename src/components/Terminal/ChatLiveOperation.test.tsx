import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { ToolCallBubble } from "../../../packages/chat-ui/src/ToolCallBubble";
import { ChatLiveOperation } from "./ChatLiveOperation";
import { projectChatTranscriptActivity } from "./chatTranscriptActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const command = "echo opaque-first; echo opaque-second";
function message(patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return {id:"opaque-command",toolId:"opaque-tool",sourceSessionId:"opaque-session",type:"tool_call",content:"",
    toolKind:"command",toolStatus:"running",toolInput:{command:`pwsh -Command '${command}'`,argv:["pwsh","-Command",command]},
    toolDetail:command,...patch};
}
function Surface({ current }: { current: NormalizedChatMessage }) {
  const live = projectChatTranscriptActivity([{msg:current}],"working",{transport:"chat_backend"},null,true).liveToolId;
  return <>
    {live !== current.id && <ToolCallBubble message={current} expanded onToggle={() => {}} />}
    <ChatLiveOperation message={live === current.id ? current : null} />
  </>;
}
let host: HTMLDivElement;
let root: Root;
beforeEach(() => { host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });
const show = async (node: ReactNode) => { await act(async () => root.render(node)); };
const click = async (node: Element | null) => {
  expect(node).not.toBeNull();
  await act(async () => { node!.dispatchEvent(new MouseEvent("click",{bubbles:true})); });
};

describe("live command presentation", () => {
  it("uses compact parsed commands, shows partial output and retains the selected segment on settlement", async () => {
    const current = message({toolResult:"opaque-partial"});
    await show(<Surface current={current} />);
    expect(host.querySelectorAll('[data-testid="chat-command-group"]')).toHaveLength(1);
    const toggle = host.querySelector('button[aria-expanded]');
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector('[aria-label="Command output"]')?.hasAttribute("hidden")).toBe(true);
    expect(host.querySelectorAll('[role="timer"]')).toHaveLength(1);
    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(host.querySelector('[aria-label="Command output"]')?.textContent).toContain("opaque-partial");
    expect(host.querySelector('[data-testid="chat-command-group-status"]')?.getAttribute("aria-label")).toBe("pending");
    const lines = host.querySelectorAll('[data-testid="chat-command-tree-line"]');
    expect(lines).toHaveLength(2);
    expect(host.textContent).not.toContain("pwsh -Command");
    await click(lines[1].querySelector("button"));
    expect(lines[1].querySelector('[data-testid="chat-command-tree-detail"]')?.textContent).toContain("echo opaque-second");
    const runningCard = host.querySelector('[data-testid="chat-command-group"]');
    await show(<Surface current={{...current,toolResult:"opaque-partial-grown"}} />);
    expect(host.querySelector('[data-testid="chat-command-group"]')).toBe(runningCard);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(host.querySelector('[aria-label="Command output"]')?.textContent).toContain("opaque-partial-grown");
    await show(<Surface current={{...current,toolResult:"opaque-final",toolStatus:"completed",toolExitCode:0}} />);
    expect(host.querySelector('[data-testid="chat-live-operation"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="chat-command-group"]')).toHaveLength(1);
    expect(host.querySelector('[data-testid="chat-command-group-status"]')?.getAttribute("aria-label")).toBe("success");
    expect(host.querySelector('[aria-label="Command output"]')?.textContent).toContain("opaque-final");
    expect(host.querySelectorAll('[data-testid="chat-command-tree-line"]')[1].querySelector('[data-testid="chat-command-tree-detail"]')?.textContent).toContain("echo opaque-second");
  });

  it.each(["failed","interrupted","abandoned"] as const)("settles %s once without assigning script segments an execution status", async status => {
    const current = message({id:`opaque-${status}`,toolId:`opaque-${status}`});
    await show(<Surface current={current} />);
    expect(host.querySelector('[data-testid="chat-command-tree-line"] [aria-label="pending"]')).toBeNull();
    await show(<Surface current={{...current,toolStatus:status,toolResult:`opaque-${status}-output`}} />);
    expect(host.querySelector('[data-testid="chat-live-operation"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="chat-command-group"]')).toHaveLength(1);
    expect(host.querySelector('[data-testid="chat-command-group-status"]')?.getAttribute("aria-label")).toBe(status === "failed" ? "error" : "interrupted");
    expect(host.textContent).toContain(`opaque-${status}-output`);
  });

  it("keeps a single opaque command interactive and exposes only supplied output", async () => {
    const current = message({id:"opaque-single",toolId:"opaque-single",toolInput:{command:"opaque-executable"},toolDetail:"opaque-executable"});
    await show(<ChatLiveOperation message={current} />);
    expect(host.querySelector('[data-testid="chat-command-group"]')).toBeNull();
    const toggle = host.querySelector('button[aria-expanded]');
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    const detailsId = toggle?.getAttribute("aria-controls");
    expect(detailsId).toBeTruthy();
    expect(document.getElementById(detailsId!)?.hidden).toBe(true);
    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(host.textContent).toContain("opaque-executable");
    expect(host.textContent).not.toContain("opaque-output");
    await show(<ChatLiveOperation message={{...current,toolResult:"opaque-output"}} />);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(host.textContent).toContain("opaque-output");
    await click(host.querySelector('button[aria-label="Hide full command"]'));
    expect(host.querySelector('button[aria-label="Show full command"]')).not.toBeNull();
    await show(<ChatLiveOperation message={{...current,toolId:"opaque-replacement",toolResult:"opaque-replacement-output"}} />);
    expect(host.querySelector('button[aria-expanded="false"]')).not.toBeNull();
    await click(host.querySelector('button[aria-expanded="false"]'));
    expect(host.textContent).toContain("opaque-replacement-output");
    await show(<ChatLiveOperation message={null} />);
    expect(host.textContent).toBe("");
  });

  it("keeps generic details collapsed until revealed and preserves the manual choice as progress changes", async () => {
    const current = message({id:"opaque-generic",toolKind:"generic",toolInput:{query:"opaque-query"},
      toolDetail:"opaque-operation-details",toolResult:"opaque-partial-result",toolStartedAtMs:1_000});
    await show(<ChatLiveOperation message={current} />);
    const toggle = host.querySelector<HTMLButtonElement>('button[aria-expanded]');
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    const detailsId = toggle?.getAttribute("aria-controls");
    expect(detailsId).toBeTruthy();
    const details = document.getElementById(detailsId!);
    expect(details?.getAttribute("role")).toBe("region");
    expect(details?.hidden).toBe(true);
    expect(host.querySelector('[data-testid="toolcard-status"]')?.getAttribute("aria-label")).toBe("pending");
    expect(host.querySelectorAll('[role="timer"]')).toHaveLength(1);
    await click(toggle);
    expect(details?.hidden).toBe(false);
    expect(details?.textContent).toContain("opaque-operation-details");
    expect(details?.textContent).toContain("opaque-partial-result");
    await show(<ChatLiveOperation message={{...current,toolResult:"opaque-grown-result",toolDetail:"opaque-updated-details"}} />);
    expect(host.querySelector('button[aria-expanded]')).toBe(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(details?.textContent).toContain("opaque-updated-details");
    expect(details?.textContent).toContain("opaque-grown-result");
    await click(toggle);
    await show(<ChatLiveOperation message={{...current,toolResult:"opaque-later-result"}} />);
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(details?.hidden).toBe(true);
    await click(toggle);
    expect(details?.textContent).toContain("opaque-later-result");
  });
});
