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

  it("renders an accessible time in the narrow-hidden chat surface gutter", async () => {
    const { host, root } = await renderTime();
    const times = host.querySelectorAll<HTMLTimeElement>("time[data-testid='chat-message-time']");
    const [agentTime, userTime] = times;
    expect(times).toHaveLength(2);
    expect(agentTime?.dateTime).toBe("2024-01-01T13:05:00.000Z");
    expect(agentTime?.classList.contains("ct-chat-message-time--hidden-when-narrow")).toBe(true);
    expect(agentTime?.classList.contains("ct-chat-message-time--left")).toBe(true);
    expect(agentTime?.closest(".ct-chat-surface .ct-chat-message-row")).toBe(
      host.querySelector("[data-testid='agent-row']"),
    );
    expect(agentTime?.textContent).toBe(formatMessageTime(Date.parse("2024-01-01T13:05:00.000Z")));
    expect(userTime?.classList.contains("ct-chat-message-time--right")).toBe(true);
    expect(userTime?.parentElement).toBe(host.querySelector("[data-testid='user-row']"));
    expect(userTime?.closest("[data-testid='user-row']")?.textContent).toContain("user bubble");
    await unmount(root);
  });
});
