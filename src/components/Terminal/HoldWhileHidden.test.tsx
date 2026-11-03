import assert from "node:assert/strict";
import { act, createElement, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { HoldWhileHidden } from "./HoldWhileHidden";

const dom = new Window({ url: "http://localhost/" });
Object.assign(globalThis, { window: dom, document: dom.document, IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement("div");
const root = createRoot(host);
let mounted = 0;
let released = 0;
function HeavyPreview() {
  useEffect(() => { mounted++; return () => { released++; }; }, []);
  return createElement("img", { src: "data:image/png;base64,opaque", alt: "preview" });
}
async function show(visible: boolean) {
  await act(async () => root.render(createElement(HoldWhileHidden, { visible, children: createElement(HeavyPreview) })));
}
await show(true);
assert.equal(mounted, 1);
await show(false);
assert.equal(released, 1);
assert.equal(host.querySelectorAll("img").length, 0);
await show(true);
assert.equal(mounted, 2);
await act(async () => root.unmount());
assert.equal(released, 2);
console.log("PASS inactive transcript releases previews and restores on activation");
