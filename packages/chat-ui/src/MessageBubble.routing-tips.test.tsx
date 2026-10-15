import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test } from "vitest";
import type { MessageSegment } from "@codeterm/chat-core";
import fixtures from "../../../tests/fixtures/message-envelope.json";
import { enrichTimelineItems } from "../../../src/components/Terminal/chatEnrichment";
import { MessageBubble } from "./MessageBubble";

test("glued routed and owner sends render separate bodies without agent tips", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const messages = (fixtures as Array<{ name: string; raw: string; segments: MessageSegment[] }>).
    filter((fixture) => ["glued routed then owner", "owner newline routed"].includes(fixture.name)).
    map((fixture) => ({ id: fixture.name, type: "user" as const, content: fixture.raw, segments: fixture.segments }));
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    for (const message of messages) {
      const entries = enrichTimelineItems([{ kind: "message", message }], new WeakMap());
      expect(entries).toHaveLength(2);
      await act(async () => root.render(<>{entries.map((entry) => <MessageBubble key={entry.msg.id} message={entry.msg} origin={entry.origin} />)}</>));
      expect(host.textContent?.split("opaque routed body")).toHaveLength(2);
      expect(host.textContent?.split("opaque owner body")).toHaveLength(2);
      expect(host.textContent).toContain("Sender");
      expect(host.textContent).not.toContain("opaque hidden hint");
      expect(host.textContent).not.toContain("<domios");
    }
  } finally {
    await act(async () => root.unmount());
  }
});
