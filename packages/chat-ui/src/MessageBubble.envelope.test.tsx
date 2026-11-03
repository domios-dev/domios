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

for (const pasted of [false, true]) {
  test(`bounds a long ${pasted ? "pasted envelope" : "plain user message"} with an inline reader under the bubble`, async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const body = "Opaque user paragraph\n".repeat(400) + "Final user sentinel";
    const raw = `<pasted_content id="opaque">\n${body}\n</pasted_content id="opaque">\n\npasted by Domios`;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const render = (suffix = "") => {
      const rows = enrichTimelineItems([{ kind: "message", message: {
        id: "long-user", type: "user", content: pasted ? raw + suffix : body + suffix,
        segments: pasted ? [{ type: "paste", id: "opaque", body: body + suffix }] : undefined,
      } }], new WeakMap());
      return <MessageBubble message={rows[0].msg} origin={rows[0].origin} />;
    };
    try {
      await act(async () => root.render(render()));
      const bubble = host.querySelector<HTMLElement>('[role="group"][aria-label="User message"]')!;
      Object.defineProperties(bubble, { scrollHeight: { value: 5910 }, clientHeight: { value: 320 } });
      await act(async () => root.render(render("\n")));
      const opener = host.querySelector<HTMLButtonElement>('button[aria-label="Open full screen: Message content"]')!;
      expect(opener).toBeTruthy();
      expect(bubble.contains(opener)).toBe(false);
      const disclosure = host.querySelector<HTMLButtonElement>('button[aria-expanded="false"]')!;
      expect(disclosure).toBeTruthy();
      await act(async () => disclosure.click());
      expect(disclosure.getAttribute("aria-expanded")).toBe("true");
      await act(async () => disclosure.click());
      expect(disclosure.getAttribute("aria-expanded")).toBe("false");
      await act(async () => opener.click());
      expect(document.querySelector('[role="dialog"] [aria-label="Reader content"]')?.textContent).toContain("Final user sentinel");
      await act(async () => document.querySelector<HTMLButtonElement>('button[aria-label="Close fullscreen viewer"]')!.click());
      expect(document.querySelector('[role="dialog"]')).toBeNull();
      expect(document.activeElement).toBe(opener);
      expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
}
