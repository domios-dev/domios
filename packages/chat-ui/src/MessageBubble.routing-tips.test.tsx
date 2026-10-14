import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test } from "vitest";
import { projectMessageParts } from "@codeterm/chat-core";
import { enrichTimelineItems } from "../../../src/components/Terminal/chatEnrichment";
import { MessageBubble } from "./MessageBubble";

test("glued routed and owner sends render separate bodies without agent tips", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const content = '<domios from="tab" label="Sender" tab="abc12345">opaque routed body <tip>opaque hidden hint</tip></domios>opaque owner body';
  const reverse = "opaque owner body" + content.slice(0, content.indexOf("</domios>") + "</domios>".length);
  const messages = [
    { id: "glued", type: "user" as const, content },
    ...projectMessageParts([{ id: "reverse", type: "user" as const, content: reverse, correlationId: "owner" }],
      [{ content: "opaque owner body", correlationId: "owner" }], "reverse"),
  ];
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
