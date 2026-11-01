import assert from "node:assert/strict";
import { act, useRef, useState } from "react";
import { Window } from "happy-dom";

const view = new Window({ url: "http://localhost/" });
const define = (name: string, value: unknown) => Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
define("IS_REACT_ACT_ENVIRONMENT", true);
define("window", view);
define("document", view.document);
define("navigator", view.navigator);
define("ResizeObserver", view.ResizeObserver);
for (const name of ["HTMLElement", "Element", "Node", "Event", "KeyboardEvent", "MouseEvent", "HTMLTextAreaElement"]) {
  define(name, (view as unknown as Record<string, unknown>)[name]);
}
async function main() {
const [{ createRoot }, { ExpandedComposer }, { dismissTopSurface }] = await Promise.all([
  import("react-dom/client"), import("./ExpandedComposer"), import("../../lib/backDismissal"),
]);
const sent: string[] = [];
function Harness() {
  const [value, setValue] = useState("opaque draft\nsecond line");
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const editor = <>
    <textarea aria-label={open ? "Expanded message" : "Inline draft"} value={value} onChange={(event) => setValue(event.target.value)} />
    <button type="button" disabled={!value.trim()} onClick={() => { sent.push(value); setValue(""); setOpen(false); }}>Send</button>
  </>;
  return <div ref={anchor}>
    <button onClick={() => setOpen(true)}>Expand</button>
    {open ? <ExpandedComposer anchorRef={anchor} onClose={() => setOpen(false)}>{editor}</ExpandedComposer> : editor}
  </div>;
}
function field(label: string) {
  const element = document.querySelector<HTMLTextAreaElement>(`textarea[aria-label="${label}"]`);
  assert.ok(element);
  return element;
}
async function click(label: string) {
  const button = [...document.querySelectorAll("button")].find((element) => element.textContent?.trim() === label || element.getAttribute("aria-label") === label);
  assert.ok(button);
  await act(async () => button.click());
}
async function type(label: string, value: string) {
  const element = field(label);
  await act(async () => {
    Object.getOwnPropertyDescriptor(view.HTMLTextAreaElement.prototype, "value")!.set!.call(element, value);
    element.dispatchEvent(new view.Event("input", { bubbles: true }) as unknown as Event);
  });
}
async function key(key: string, options: { ctrlKey?: boolean; isComposing?: boolean } = {}) {
  await act(async () => {
    field("Expanded message").dispatchEvent(new view.KeyboardEvent("keydown", {
      key, bubbles: true, cancelable: true, ...options,
    }) as unknown as Event);
  });
}
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
try {
  await act(async () => root.render(<Harness />));
  await click("Expand");
  assert.equal(document.querySelector('[role="dialog"]')?.getAttribute("aria-modal"), "true");
  assert.ok(document.activeElement === field("Expanded message"), "expanded editor receives focus");
  await type("Expanded message", "edited in large editor\nkept on close");
  assert.equal(document.querySelectorAll("textarea").length, 1);
  await key("Enter");
  assert.deepEqual(sent, []);
  await key("Enter", { ctrlKey: true, isComposing: true });
  assert.deepEqual(sent, []);
  await key("Escape");
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(field("Inline draft").value, "edited in large editor\nkept on close");
  await type("Inline draft", "edited in inline editor");
  await click("Expand");
  assert.equal(field("Expanded message").value, "edited in inline editor");
  await act(async () => { assert.equal(dismissTopSurface(), true); });
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(field("Inline draft").value, "edited in inline editor");
  await click("Expand");
  await click("Send");
  assert.deepEqual(sent, ["edited in inline editor"]);
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(field("Inline draft").value, "");
  await click("Expand");
  await key("Enter", { ctrlKey: true });
  assert.equal(sent.length, 1);
  await click("Close expanded editor");
  console.log("Expanded composer reused editor, shared draft, close/back and submit passed");
} finally {
  await act(async () => root.unmount());
  host.remove();
  await view.happyDOM.close();
}
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
