import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test } from "vitest";
import type { MessageSegment } from "@codeterm/chat-core";
import { enrichTimelineItems } from "../../../src/components/Terminal/chatEnrichment";
import { MessageBubble } from "./MessageBubble";
import fixtureData from "../../../tests/fixtures/message-envelope.json";

test("merged user and routed turn renders a user bubble and a named routed card", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const fixtures = fixtureData as Array<{ name: string; raw: string; segments: MessageSegment[] }>;
  const fixture = fixtures.find((entry) => entry.name === "owner merged screenshot")!;
  const rows = enrichTimelineItems([{ kind: "message", message: { id: "merged", type: "user", content: fixture.raw, segments: fixture.segments } }], new WeakMap());
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<>{rows.map((row) => <MessageBubble key={row.msg.id} message={row.msg} origin={row.origin} />)}</>));
    const user = host.querySelectorAll('[role="group"][aria-label="User message"]');
    const routed = host.querySelectorAll('[role="group"][aria-label="Received from Probe"]');
    expect(user).toHaveLength(1);
    expect(routed).toHaveLength(1);
    expect(user[0].textContent?.trim()).toBe("какой щас план?");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

test("routed card renders Rust attachment segments beside its body", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const fixture = (fixtureData as Array<{ name: string; raw: string; segments: MessageSegment[] }>).find((entry) => entry.name === "routed attachment body")!;
  const rows = enrichTimelineItems([{ kind: "message", message: { id: "attachment", type: "user", content: fixture.raw, segments: fixture.segments } }], new WeakMap());
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MessageBubble message={rows[0].msg} origin={rows[0].origin} />));
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Expand tool details"]')?.click());
    expect(host.querySelector('button[aria-label="Open image image.png"]')).not.toBeNull();
    expect(host.textContent).toContain("note");
    expect(host.textContent).toContain("done");
  } finally {
    await act(async () => root.unmount());
  }
});
