import React, { act, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ChatBlockHostProvider } from "./blocks/ChatBlockHost";
import { renderChatCardMenu } from "../../../src/components/Terminal/ChatCardMenu";
import { ThoughtLine } from "./toolcard/ThoughtLine";
import { CompactToolRowsProvider, useCompactToolRow } from "./useCompactToolRow";
import { ChatNoticeCard } from "./ChatNoticeCard";
import { ChatBlockFrame } from "./blocks/ChatBlockFrame";
import { ToolCardFrame } from "./toolcard/ToolCardFrame";
import { BoundedMessageContent } from "./BoundedMessageContent";
import { ExpandableReader } from "./ExpandableReader";
import { InboundReportCard } from "./codeterm/InboundReportCard";
import { ToolActivityStream } from "./toolcard/ToolActivityStream";
import type { ToolEvent } from "@codeterm/chat-core";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
let host: HTMLDivElement;
async function mount(content: React.ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(content));
}
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
function button(name: string) {
  const result = [...document.querySelectorAll("button")].find(node => node.getAttribute("aria-label") === name || node.textContent === name);
  expect(result).toBeTruthy();
  return result!;
}

function CompactSurface({ children }: { children: React.ReactNode }) {
  const surface = useRef<HTMLDivElement>(null);
  return <CompactToolRowsProvider surfaceRef={surface}><div ref={surface} className="ct-chat-surface">{children}</div></CompactToolRowsProvider>;
}

it("shares one surface observer and pointer listener across tool rows and releases them", async () => {
  let width = 900;
  let resize: ResizeObserverCallback | undefined;
  const observe = vi.fn();
  const disconnect = vi.fn();
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe = observe;
    disconnect = disconnect;
  });
  const media = window.matchMedia("(pointer: coarse)");
  Object.defineProperty(media, "matches", { value: false });
  const addListener = vi.spyOn(media, "addEventListener");
  const removeListener = vi.spyOn(media, "removeEventListener");
  const matchMedia = vi.spyOn(window, "matchMedia").mockReturnValue(media);
  const bounds = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
    return { ...bounds.call(this), width };
  });
  function Consumer() {
    return <button disabled={!useCompactToolRow()} />;
  }
  await mount(<CompactSurface>{Array.from({ length: 100 }, (_, index) => <Consumer key={index} />)}</CompactSurface>);
  expect(observe).toHaveBeenCalledTimes(1);
  expect(matchMedia).toHaveBeenCalledTimes(1);
  expect(addListener).toHaveBeenCalledTimes(1);
  expect([...host.querySelectorAll("button")].every(control => control.disabled)).toBe(true);
  width = 380;
  await act(async () => resize?.([], {} as ResizeObserver));
  expect([...host.querySelectorAll("button")].every(control => !control.disabled)).toBe(true);
  await act(async () => root.render(null));
  expect(disconnect).toHaveBeenCalledTimes(1);
  expect(removeListener).toHaveBeenCalledWith("change", addListener.mock.calls[0][1]);
});

it("retains compact pending and error status without adding action targets", async () => {
  const media = window.matchMedia("(pointer: coarse)");
  Object.defineProperty(media, "matches", { value: true });
  vi.spyOn(window, "matchMedia").mockReturnValue(media);
  let toggles = 0;
  await mount(<CompactSurface><ChatBlockHostProvider value={{ renderMenu: renderChatCardMenu }}>
    {(["pending", "error", "success"] as const).map(status => {
      const event: ToolEvent = { id: status, phase: "result", status, kind: "search", message: { id: status, type: "tool_call", content: "" } };
      return <ToolCardFrame key={status} event={event} expanded={false} onToggle={() => { toggles += 1; }}><p>Opaque detail</p></ToolCardFrame>;
    })}
  </ChatBlockHostProvider></CompactSurface>);
  expect(host.querySelectorAll('[role="status"]')).toHaveLength(2);
  for (const status of ["pending", "error"]) {
    const indicator = host.querySelector(`[role="status"][aria-label="${status}"]`);
    expect(indicator?.querySelector("svg")).toBeTruthy();
    expect(indicator?.closest("button")).toBeNull();
    await act(async () => indicator?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  }
  expect(toggles).toBe(2);
});

for (const input of ["coarse", "narrow"] as const) {
  it(`opens the ${input} tool reader from its first menu action and restores kebab focus`, async () => {
    const matchMedia = window.matchMedia.bind(window);
    vi.spyOn(window, "matchMedia").mockImplementation(query => {
      const media = matchMedia(query);
      if (query === "(pointer: coarse)") Object.defineProperty(media, "matches", { value: input === "coarse" });
      return media;
    });
    const bounds = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
      const rect = bounds.call(this);
      return this.classList.contains("ct-chat-surface") ? { ...rect, width: input === "narrow" ? 380 : 900 } : rect;
    });
    let toggles = 0;
    const event: ToolEvent = { id: "compact", phase: "result", status: "success", kind: "search", result: "opaque output", message: { id: "compact", type: "tool_call", content: "" } };
    await mount(<CompactSurface><ChatBlockHostProvider value={{ renderMenu: renderChatCardMenu }}>
      <ToolCardFrame event={event} title="Opaque compact row" expanded={false} onToggle={() => { toggles += 1; }}><p>Opaque detail</p></ToolCardFrame>
    </ChatBlockHostProvider></CompactSurface>);
    const toggle = button("Expand tool details");
    await act(async () => toggle.click());
    expect(toggles).toBe(1);
    expect(host.querySelector('[aria-label="Expand card"]')).toBeNull();
    expect(host.querySelector('[aria-label^="Open full screen:"]')).toBeNull();
    const kebab = button("Actions for Opaque compact row");
    for (const close of ["Escape", "button"]) {
      await act(async () => kebab.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
      const first = document.querySelector<HTMLButtonElement>('[role="menuitem"]')!;
      expect(first).toBeTruthy();
      expect(first.querySelector("svg")).toBeTruthy();
      await act(async () => first.click());
      const dialog = document.querySelector('[role="dialog"][aria-label="Opaque compact row"]');
      expect(dialog?.textContent).toContain("Opaque detail");
      expect(toggles).toBe(1);
      if (close === "Escape") await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
      else await act(async () => button("Close fullscreen viewer").click());
      expect(document.querySelector('[role="dialog"]')).toBeNull();
      expect(document.activeElement).toBe(kebab);
    }
  });
}

it("keeps the Thinking reader independent of the disclosure handler", async () => {
  let toggles = 0;
  await mount(<ThoughtLine text={"Opaque thought\n".repeat(10)} expanded={false} onToggle={() => { toggles += 1; }} />);
  const opener = button("Open full screen: Thinking");
  await act(async () => opener.click());
  expect(toggles).toBe(0);
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain("Opaque thought");
  await act(async () => button("Close fullscreen viewer").click());
  expect(document.activeElement).toBe(opener);
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-controls]')!.click());
  expect(toggles).toBe(1);
});

it("keeps the same interactive content through open, Escape, close and focus return", async () => {
  function Content() {
    const [count, setCount] = useState(0);
    return <button onClick={() => setCount(count + 1)}>Count {count}</button>;
  }
  let opens = 0;
  await mount(<ExpandableReader label="Opaque content" openerPlacement="header" onOpen={() => { opens += 1; }}>
    {(_, opener) => <><header>{opener}</header><Content /></>}
  </ExpandableReader>);
  const original = button("Count 0");
  await act(async () => original.click());
  const opener = button("Open full screen: Opaque content");
  expect(opener.title).toBe("Open full screen");
  await act(async () => opener.click());
  const dialog = document.querySelector('[role="dialog"][aria-label="Opaque content"]');
  expect(dialog?.contains(original)).toBe(true);
  expect(dialog?.querySelector<HTMLElement>('[aria-label="Reader content"]')?.tabIndex).toBe(0);
  expect(original.textContent).toBe("Count 1");
  expect(document.activeElement).toBe(button("Close fullscreen viewer"));
  await act(async () => window.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(document.activeElement).toBe(opener);
  await act(async () => opener.click());
  await act(async () => button("Close fullscreen viewer").click());
  expect(document.activeElement).toBe(opener);
  expect(button("Count 1")).toBe(original);
  expect(opens).toBe(2);
});

it("reads the entire report including its final table row inline and in fullscreen", async () => {
  const rows = Array.from({ length: 90 }, (_, i) => `| row ${i} | opaque value ${i} |`).join("\n");
  await mount(<InboundReportCard sender="Lens" expanded onToggle={() => {}}
    message={{ id: "reader-report", type: "user", content: `status: in_review\n\n| Key | Value |\n| --- | --- |\n${rows}\n\nFinal report sentinel` }} />);
  const region = host.querySelector<HTMLElement>('[role="region"]')!;
  Object.defineProperties(region, { scrollHeight: { configurable: true, value: 2000 }, clientHeight: { configurable: true, value: 160 } });
  await act(async () => root.render(<InboundReportCard sender="Lens" expanded onToggle={() => {}}
    message={{ id: "reader-report", type: "user", content: `status: in_review\n\n| Key | Value |\n| --- | --- |\n${rows}\n\nFinal report sentinel\n` }} />));
  await act(async () => button("Show all").click());
  expect(region.style.maxHeight).toBe("none");
  expect(region.textContent).toContain("opaque value 89");
  expect(region.textContent).toContain("Final report sentinel");
  const table = region.querySelector("table");
  await act(async () => button("Open full screen: Report from Lens").click());
  const dialog = document.querySelector('[role="dialog"][aria-label="Report from Lens"]');
  expect(dialog?.contains(table!)).toBe(true);
  expect(dialog?.textContent).toContain("Final report sentinel");
  await act(async () => button("Close fullscreen viewer").click());
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

it("opens a collapsed tool group with every member available", async () => {
  const member: ToolEvent = { id: "member", phase: "result", status: "success", kind: "generic", summary: "Opaque final member", message: { id: "member", type: "tool_call", content: "" } };
  const event: ToolEvent = { ...member, id: "stream", stream: { flowId: "reader", members: [member], entries: [{ kind: "action", event: member }], active: false } };
  await mount(<ToolActivityStream event={event} expanded={false} onToggle={() => {}} renderAction={(item) => <p>{item.summary}</p>} />);
  expect(host.querySelector('[aria-label="Tool actions"]')?.getAttribute("hidden")).not.toBeNull();
  await act(async () => button("Open full screen: Tool activity").click());
  const dialog = document.querySelector('[role="dialog"][aria-label="Tool activity"]');
  expect(dialog?.textContent).toContain("Opaque final member");
  expect(dialog?.querySelector('[aria-label="Tool actions"]')?.hasAttribute("hidden")).toBe(false);
  await act(async () => button("Close fullscreen viewer").click());
  expect(host.querySelector('[aria-label="Tool actions"]')?.hasAttribute("hidden")).toBe(true);
});

for (const surface of ["tool", "block", "notice"] as const) {
  it(`opens the ${surface} reader independently and returns focus to its only opener`, async () => {
    let toggles = 0;
    const toggle = () => { toggles += 1; };
    const content = <BoundedMessageContent collapsedHeight="100px"><p>Opaque detail</p></BoundedMessageContent>;
    const event: ToolEvent = { id: "reader-row", phase: "result", status: "success", kind: "generic", summary: "Opaque summary", message: { id: "reader-row", type: "tool_call", content: "" } };
    await mount(surface === "tool"
      ? <ToolCardFrame event={event} title="Opaque row" expanded={false} onToggle={toggle}>{content}</ToolCardFrame>
      : surface === "block"
        ? <ChatBlockFrame name="opaque" source="opaque-source" title="Opaque row" icon={null} details={content} />
        : <ChatNoticeCard testId="opaque-notice" title="Opaque row" icon={null} expanded={false} onToggle={toggle}>{content}</ChatNoticeCard>);
    const opener = button("Open full screen: Opaque row");
    expect(host.querySelectorAll('[aria-label^="Open full screen:"]')).toHaveLength(1);
    expect(opener.parentElement?.closest("button")).toBeNull();
    await act(async () => opener.click());
    expect(toggles).toBe(0);
    const dialog = document.querySelector('[role="dialog"][aria-label="Opaque row"]');
    expect(dialog?.textContent).toContain("Opaque detail");
    expect(dialog?.querySelectorAll('button:not([hidden])[aria-label^="Open full screen:"]')).toHaveLength(0);
    await act(async () => button("Close fullscreen viewer").click());
    expect(document.activeElement).toBe(opener);
  });
}
