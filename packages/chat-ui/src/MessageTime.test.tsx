import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";

import { formatMessageTime, MessageTime } from "./MessageTime";

async function renderTime(): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <div className="ct-chat-surface">
        <div className="ct-chat-message-row" data-testid="agent-row">
          <MessageTime timestamp="2024-01-01T13:05:00.000Z" side="left" />
          <div className="ct-chat-message-column" data-testid="agent-message">agent content</div>
        </div>
        <div className="ct-chat-message-row" data-testid="user-row">
          <MessageTime timestamp="2024-01-01T13:06:00.000Z" side="right" />
          <div className="ct-chat-message-column" data-testid="user-message">user bubble</div>
        </div>
      </div>,
    );
  });
  return { host, root };
}

async function unmount(root: Root): Promise<void> {
  await act(async () => root.unmount());
}

describe("chat message time", () => {
  it("uses each locale's 12 or 24 hour clock", () => {
    const timestamp = new Date(2024, 0, 1, 13, 5).getTime();
    expect(formatMessageTime(timestamp, "en-US")).toBe("1:05 PM");
    expect(formatMessageTime(timestamp, "en-GB")).toBe("13:05");
  });

  it("renders accessible times with the supplied timestamps", async () => {
    const { host, root } = await renderTime();
    try {
      const times = host.querySelectorAll<HTMLTimeElement>("time");
      expect(times).toHaveLength(2);
      expect(times[0].dateTime).toBe("2024-01-01T13:05:00.000Z");
      expect(times[1].dateTime).toBe("2024-01-01T13:06:00.000Z");
      for (const time of times) expect(time.getAttribute("aria-label")).toBeTruthy();
    } finally {
      await unmount(root);
      host.remove();
    }
  });
});
