import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { MessageBubble } from "./MessageBubble";
import { MessageTimeFrame } from "./MessageTimeFrame";
import { FIRST_TIME, timeFixtures } from "./messageTimeFixtures.testdata";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("message hover time wiring", () => {
  it.each(timeFixtures)("reveals the time when hovering $name", async ({ message, item, origin, streaming }) => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<MessageBubble message={message} item={item} origin={origin} streaming={streaming} />));
      expect(host.querySelector("time")).toBeNull();
      const row = host.firstElementChild!;
      await act(async () => row.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })));
      expect(host.querySelector("time")?.dateTime).toBe(FIRST_TIME);
      expect(host.querySelector("time")?.getAttribute("aria-label")).toBeTruthy();
      await act(async () => row.dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body })));
      expect(host.querySelector("time")).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
  it.each(["outbound report", "notification ask"])("reveals a projected %s event on focus", async (name) => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<MessageTimeFrame timestamp={FIRST_TIME} side="left"><button>{name}</button></MessageTimeFrame>));
      await act(async () => host.querySelector("button")!.focus());
      expect(host.querySelector("time")?.dateTime).toBe(FIRST_TIME);
      expect(host.querySelector("time")?.getAttribute("aria-label")).toBeTruthy();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
