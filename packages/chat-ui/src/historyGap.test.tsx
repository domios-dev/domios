import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";
import { HistoryGapProvider, normalizedTimelineMarker } from "./timelineMarkers";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const gap: NormalizedChatMessage = {
  id: "gap-fixture",
  type: "system",
  content: "fixture-gap-text",
  timelineMarker: { kind: "history_gap", source: "chat_backend", session_id: "fixture-session", before_offset: 7 },
};

let mounted: { host: HTMLElement; root: Root } | null = null;

async function mount(onLoad?: (message: NormalizedChatMessage) => Promise<void>): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  mounted = { host, root };
  await act(async () => {
    root.render(<HistoryGapProvider onLoad={onLoad}><MessageBubble message={gap} /></HistoryGapProvider>);
  });
  return host;
}

afterEach(() => {
  act(() => mounted?.root.unmount());
  mounted?.host.remove();
  mounted = null;
});

describe("history gap row", () => {
  it("accepts only a well-formed gap marker", () => {
    expect(normalizedTimelineMarker(gap)).toEqual(gap.timelineMarker);
    for (const before of [0, -1, 1.5]) {
      expect(normalizedTimelineMarker({
        ...gap,
        timelineMarker: { kind: "history_gap", source: "chat_backend", session_id: "fixture-session", before_offset: before },
      })).toBeNull();
    }
  });

  it("is an explicit boundary whose control loads the page behind it", async () => {
    const requested: string[] = [];
    const host = await mount(async (message) => { requested.push(message.id); });
    const region = host.querySelector('[role="group"][aria-label="Earlier history not loaded"]');
    expect(region?.textContent).toContain("fixture-gap-text");
    const button = region?.querySelector("button");
    await act(async () => { button?.click(); });
    expect(requested).toEqual(["gap-fixture"]);
  });

  it("reports a failed load and offers a retry", async () => {
    let attempts = 0;
    let settled: Promise<void> = Promise.resolve();
    const host = await mount(() => {
      attempts += 1;
      settled = Promise.reject(new Error("fixture"));
      return settled;
    });
    const button = () => host.querySelector("button")!;
    const clickAndSettle = () => act(async () => {
      button().click();
      await settled.catch(() => {});
    });
    await clickAndSettle();
    expect(button().disabled).toBe(false);
    await clickAndSettle();
    expect(attempts).toBe(2);
  });

  it("stays visible without a loader", async () => {
    const host = await mount();
    expect(host.querySelector('[role="group"]')).not.toBeNull();
    expect(host.querySelector("button")).toBeNull();
  });
});
