import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { ChatDaySeparator, chatDaySeparators, MessageTimeFrame } from "@codeterm/chat-ui";
import { MessageActionsDot } from "./MessageActionsMenu";
import { useChatLayoutInsets } from "./useChatLayoutInsets";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("shows supplied time and full date in the message menu", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const metadata = { time: "time-token", date: "date-token", dateTime: "2026-10-14T12:00:00.000Z", sender: "sender-token", deliveryState: "state-token" };
  try {
    await act(async () => root.render(<MessageActionsDot clip={{ id: "m", role: "user", author: metadata.sender, source: "payload" }} metadata={metadata} side="end" open onOpenChange={() => {}} actions={{ quote: () => {}, ask: () => {}, select: () => {} }} />));
    const menu = host.querySelector("[role='menu']")!;
    expect(menu.querySelector("time")?.dateTime).toBe(metadata.dateTime);
    expect(menu.querySelector("time")?.textContent).toContain(metadata.time);
    expect(menu.querySelector("time")?.textContent).toContain(metadata.date);
    expect(menu.querySelector("[role='note']")?.textContent).toContain(metadata.sender);
    expect(menu.querySelector("[role='note']")?.textContent).toContain(metadata.deliveryState);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("renders separators from the day model between consecutive local days", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const labels = { today: "current-token", yesterday: "previous-token" };
  const separators = chatDaySeparators([Date.parse("2026-10-13T12:00:00Z"), Date.parse("2026-10-13T13:00:00Z"), Date.parse("2026-10-14T12:00:00Z")], { now: Date.parse("2026-10-14T15:00:00Z"), timeZone: "UTC", labels });
  try {
    await act(async () => root.render(<>{[0, 1, 2].map((index) => <div key={index}><ChatDaySeparator label={separators.get(index)} /><article aria-label={String(index)} /></div>)}</>));
    expect([...host.querySelectorAll("[role='separator']")].map((node) => node.getAttribute("aria-label"))).toEqual([labels.yesterday, labels.today]);
    expect(host.children[1].querySelector("[role='separator']")).toBeNull();
    expect(host.children[2].firstElementChild?.getAttribute("role")).toBe("separator");
  } finally { await act(async () => root.unmount()); }
});

it("hides the side time gutter below the breakpoint and restores it on resize", async () => {
  let width = 639;
  let resized = () => {};
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resized = callback; }
    observe() {}
    disconnect() {}
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => ({ width, height: 400, top: 0, bottom: 400, left: 0, right: width, x: 0, y: 0, toJSON: () => ({}) }));
  function Surface() {
    const rootRef = useRef<HTMLDivElement>(null);
    useChatLayoutInsets(rootRef);
    return <div ref={rootRef}><div role="log" className="ct-chat-surface"><MessageTimeFrame timestamp="2026-10-14T12:00:00Z" side="left"><button>fixture</button></MessageTimeFrame></div><div className="ct-chat-bottom-bar" /></div>;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Surface />));
    await act(async () => host.querySelector("button")!.focus());
    const time = host.querySelector("time")!;
    expect(time).toBeTruthy();
    expect(time.hidden).toBe(true);
    width = 640;
    await act(async () => resized());
    expect(time.hidden).toBe(false);
  } finally { await act(async () => root.unmount()); host.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); }
});
