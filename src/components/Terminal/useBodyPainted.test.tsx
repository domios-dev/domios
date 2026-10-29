import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { hasPaintedContent, useBodyPainted } from "./useBodyPainted";

it("treats chat rows and non-empty terminal rows as painted content", () => {
  const root = document.createElement("div");
  expect(hasPaintedContent(root)).toBe(false);
  root.innerHTML = '<div class="xterm-rows"><div> </div></div>';
  expect(hasPaintedContent(root)).toBe(false);
  root.innerHTML = '<div class="xterm-rows"><div>opaque-line</div></div>';
  expect(hasPaintedContent(root)).toBe(true);
  root.innerHTML = '<div data-chat-window-row="m1"></div>';
  expect(hasPaintedContent(root)).toBe(true);
  root.innerHTML = '<section data-chat-ready="false"><div data-chat-window-row="prior"></div></section>';
  expect(hasPaintedContent(root)).toBe(false);
  root.querySelector("section")!.setAttribute("data-chat-ready", "true");
  expect(hasPaintedContent(root)).toBe(true);
});

it("accepts an acknowledged blank terminal and rejects hidden or unready terminal text", () => {
  const root = document.createElement("div");
  root.innerHTML = '<section data-terminal-visible="true" data-terminal-ready="false" style="visibility:hidden"><div class="xterm-rows">prior text</div></section>';
  expect(hasPaintedContent(root)).toBe(false);
  const terminal = root.querySelector<HTMLElement>("section")!;
  terminal.setAttribute("data-terminal-ready", "true");
  expect(hasPaintedContent(root)).toBe(false);
  terminal.style.visibility = "visible";
  terminal.querySelector(".xterm-rows")!.textContent = "";
  expect(hasPaintedContent(root)).toBe(true);
});

it("reports painted once content appears after mount", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const seen: boolean[] = [];
  function Probe() {
    const ref = useRef<HTMLDivElement>(null);
    seen.push(useBodyPainted(ref, "tab-1"));
    return <div ref={ref} data-testid="body" />;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Probe />));
  expect(seen[seen.length - 1]).toBe(false);
  await act(async () => {
    const row = document.createElement("div");
    row.setAttribute("data-chat-window-row", "m1");
    host.querySelector('[data-testid="body"]')!.append(row);
  });
  expect(seen[seen.length - 1]).toBe(true);
  await act(async () => root.unmount());
  host.remove();
});
