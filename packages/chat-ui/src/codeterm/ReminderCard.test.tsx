import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";

import { ReminderCard } from "./ReminderCard";
import { relativeTime } from "./relativeTime";

async function renderCard(props: Parameters<typeof ReminderCard>[0]): Promise<{
  host: HTMLDivElement;
  root: Root;
}> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(<ReminderCard {...props} />));
  return { host, root };
}

async function unmount(root: Root): Promise<void> {
  await act(async () => root.unmount());
}

describe("ReminderCard", () => {
  it("renders creator, time and reminder content", async () => {
    const now = Date.now();
    const { host, root } = await renderCard({
      whoSet: "Main",
      model: "opus",
      setAt: String(now - 10 * 60_000),
      fireAt: String(now),
      id: "dmsg-7",
      body: "water the plants",
    });
    const text = host.textContent ?? "";
    expect(host.querySelector('[data-testid="chat-reminder-card"]')).not.toBeNull();
    expect(text).toContain("Reminder");
    expect(text).toContain("from Main");
    expect(text).toContain("water the plants");
    expect(text).toContain("set ");
    expect(text).toContain("fired ");
    expect(host.querySelector('[data-reminder-id="dmsg-7"]')).not.toBeNull();
    await unmount(root);
  });

  it("renders without creator metadata", async () => {
    const { host, root } = await renderCard({ body: "bare reminder" });
    const text = host.textContent ?? "";
    expect(text).toContain("Reminder");
    expect(text).not.toContain("from ");
    expect(text).toContain("bare reminder");
    await unmount(root);
  });

  it("shows recurrence details and stops the series by id", async () => {
    let stopped = "";
    const { host, root } = await renderCard({
      body: "watch the queue",
      id: "series-1",
      everyMs: "600000",
      nextAt: String(Date.now() + 60_000),
      skippedTicks: "2",
      onStop: (id) => { stopped = id; },
    });
    expect(host.textContent).toContain("repeats every 10m");
    expect(host.textContent).toContain("next at");
    expect(host.textContent).toContain("skipped 2");
    const stop = host.querySelector<HTMLButtonElement>('button[aria-label="Stop recurring reminder"]');
    expect(stop).not.toBeNull();
    stop?.click();
    expect(stopped).toBe("series-1");
    await unmount(root);
  });

  it("formats relative times for past, future and invalid inputs", () => {
    const now = 1_000_000_000_000;
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5m ago");
    expect(relativeTime(now + 2 * 3_600_000, now)).toBe("in 2h");
    expect(relativeTime(now, now)).toBe("just now");
    expect(relativeTime(undefined)).toBeNull();
    expect(relativeTime("not-a-number")).toBeNull();
  });
});
