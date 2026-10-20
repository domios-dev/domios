import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { useLatestCallback } from "./useLatestCallback";

const testWindow = new Window();
Object.assign(globalThis, {
  window: testWindow,
  document: testWindow.document,
  IS_REACT_ACT_ENVIRONMENT: true,
});

let handler: (value: number) => number;
function Harness({ multiplier, identity }: { multiplier: number; identity: string }) {
  handler = useLatestCallback((value: number) => value * multiplier, [identity]);
  return null;
}

async function main() {
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(createElement(Harness, { multiplier: 2, identity: "first" })));
    const first = handler!;
    assert.equal(first(3), 6);
    await act(async () => root.render(createElement(Harness, { multiplier: 4, identity: "first" })));
    assert.equal(handler!, first);
    assert.equal(first(3), 12);
    await act(async () => root.render(createElement(Harness, { multiplier: 5, identity: "next" })));
    assert.notEqual(handler!, first);
    assert.equal(handler!(3), 15);
    assert.equal(first(3), 15);
  } finally {
    await act(async () => root.unmount());
    await testWindow.happyDOM.close();
  }
}

test("hook forwards current values while preserving dependency-based identities", main);
