import assert from "node:assert/strict";
import { act, useState } from "react";
import { Window } from "happy-dom";

const view = new Window({ url: "http://localhost/" });
const define = (name: string, value: unknown) => Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
define("IS_REACT_ACT_ENVIRONMENT", true);
define("window", view);
define("document", view.document);
define("navigator", view.navigator);
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
  return <>
    <textarea aria-label="Inline draft" value={value} onChange={(event) => setValue(event.target.value)} />
    <button onClick={() => setOpen(true)}>Expand</button>
    {open && <ExpandedComposer value={value} onChange={setValue} onClose={() => setOpen(false)}
      sendDisabled={!value.trim()} onSend={() => { sent.push(value); setValue(""); setOpen(false); }} />}
  </>;
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
  assert.equal(document.activeElement, field("Expanded message"));
  await type("Expanded message", "edited in large editor\nkept on close");
  assert.equal(field("Inline draft").value, "edited in large editor\nkept on close");
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
  console.log("Expanded composer shared draft, close/back and once-only submit passed");
} finally {
  await act(async () => root.unmount());
  host.remove();
  await view.happyDOM.close();
}
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
