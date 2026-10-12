import assert from "node:assert/strict";
import test from "node:test";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { MessageBubble } from "./MessageBubble";
import { messageParts } from "./messageParts";

const text = "**opaque-emphasis**\n\n- opaque-item-a\n- opaque-item-b";

test("markdown block identities survive a growing assistant message", () => {
  const initial = messageParts("live", text);
  const grown = messageParts("live", `${text}\n\nAnother **opaque-emphasis**.`);
  assert.deepEqual(grown.slice(0, initial.length), initial);
  assert.equal(grown.length, initial.length + 1);
});

test("assistant markdown structure stays identical while running and finished", async () => {
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
  for (const key of ["window", "document", "navigator", "HTMLElement", "SVGElement", "Element", "Node"] as const) {
    const value = key === "window" ? window : key === "document" ? window.document : key === "navigator" ? window.navigator : window[key];
    Object.defineProperty(globalThis, key, { value, configurable: true });
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (content: string, streaming: boolean) => act(async () => {
    root.render(<MessageBubble message={{ id: "live", type: "assistant", content, timestamp: "2026-10-11T08:00:00.000Z" }} streaming={streaming} />);
  });
  try {
    await render(text, true);
    assert.equal(host.querySelectorAll("strong").length, 1);
    assert.equal(host.querySelectorAll("ul > li").length, 2);
    await render(`${text}\n\nAnother **opaque-emphasis**.`, true);
    assert.equal(host.querySelectorAll("strong").length, 2);
    const runningStructure = host.innerHTML;
    await render(`${text}\n\nAnother **opaque-emphasis**.`, false);
    assert.equal(host.innerHTML, runningStructure);
    await act(async () => host.querySelector(".ct-chat-message-row")!.dispatchEvent(new window.MouseEvent("mouseover", { bubbles: true })));
    assert.equal(host.querySelector("time")?.dateTime, "2026-10-11T08:00:00.000Z");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
