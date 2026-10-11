import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { MessageBubble } from "./MessageBubble";

test("streaming text stays visible until deferred markdown commits", async () => {
  const window = new Window({ url: "http://localhost/" });
  for (const [key, value] of Object.entries({ window, document: window.document, navigator: window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    Object.defineProperty(globalThis, key, { configurable: true, value });
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  const layouts: number[] = [];
  const onContentLayout = () => { layouts.push(host.querySelectorAll("strong").length); };
  const message = { id: "answer", type: "assistant" as const, timestamp: "2026-10-11T08:00:00.000Z", content: "First **answer** 🌲" };
  try {
    await act(async () => root.render(<MessageBubble message={message} streaming onContentLayout={onContentLayout} />));
    assert.equal(host.textContent, message.content);
    assert.equal(host.querySelector("strong"), null);
    const grown = { ...message, content: message.content + "\n\nSecond **answer**" };
    await act(async () => root.render(<MessageBubble message={grown} streaming onContentLayout={onContentLayout} />));
    assert.equal(host.textContent, grown.content);
    await act(async () => {
      layouts.length = 0;
      flushSync(() => root.render(<MessageBubble message={grown} streaming={false} onContentLayout={onContentLayout} />));
      assert.deepEqual(layouts, [0]);
      assert.equal(host.textContent, grown.content);
      assert.equal(host.querySelector("strong"), null);
    });
    assert.equal(host.querySelectorAll("strong").length, 2);
    assert.deepEqual(layouts, [0, 2]);
    assert.equal(host.textContent, "First answer 🌲\nSecond answer");
    await act(async () => host.firstElementChild!.dispatchEvent(new window.MouseEvent("mouseover", { bubbles: true })));
    assert.equal(host.querySelector("time")?.dateTime, message.timestamp);
    assert.deepEqual(layouts, [0, 2]);
  } finally {
    await act(async () => root.unmount());
  }
});
