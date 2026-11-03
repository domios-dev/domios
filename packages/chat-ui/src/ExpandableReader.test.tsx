import React, { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
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
});
function button(name: string) {
  const result = [...document.querySelectorAll("button")].find(node => node.getAttribute("aria-label") === name || node.textContent === name);
  expect(result).toBeTruthy();
  return result!;
}

it("keeps the same interactive content through open, Escape, close and focus return", async () => {
  function Content() {
    const [count, setCount] = useState(0);
    return <button onClick={() => setCount(count + 1)}>Count {count}</button>;
  }
  await mount(<ExpandableReader label="Opaque content">{() => <Content />}</ExpandableReader>);
  const original = button("Count 0");
  await act(async () => original.click());
  const opener = button("Open full screen: Opaque content");
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
