// Run: npx tsx src/components/shared/FormDialog.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/", width: 1600, height: 900 });
const define = (name: string, value: unknown) => Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
define("IS_REACT_ACT_ENVIRONMENT", true);
define("window", window);
define("document", window.document);
define("navigator", window.navigator);
for (const name of ["HTMLElement", "HTMLInputElement", "Element", "Node", "Event", "KeyboardEvent", "MouseEvent", "FocusEvent", "InputEvent"]) {
  define(name, (window as unknown as Record<string, unknown>)[name]);
}

// happy-dom has no layout, so the dialog reports where centring would put it for its current content.
let centredTop = 0;
const prototype = window.HTMLElement.prototype as unknown as { getBoundingClientRect(this: HTMLElement): DOMRect };
const rect = prototype.getBoundingClientRect;
prototype.getBoundingClientRect = function (this: HTMLElement) {
  const box = rect.call(this);
  return this.getAttribute("role") === "dialog" ? { ...box, top: centredTop, y: centredTop } as DOMRect : box;
};

const modules = Promise.all([import("react-dom/client"), import("./FormDialog")]);

function dialogTop(): string {
  const dialog = document.body.querySelector<HTMLElement>("[role='dialog']");
  if (!dialog) throw new Error("no dialog");
  return dialog.style.marginTop;
}

async function run() {
  const [{ createRoot }, { FormDialog }] = await modules;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (rows: number) => root.render(
    <FormDialog titleId="fixture-title" onCancel={() => {}} onSubmit={(event) => event.preventDefault()}>
      <h2 id="fixture-title">Fixture</h2>
      {Array.from({ length: rows }, (_, index) => <p key={index}>row</p>)}
    </FormDialog>,
  );

  centredTop = 300;
  await act(async () => render(1));
  assert.equal(dialogTop(), "300px", "opens where centring places it");

  centredTop = 280;
  await act(async () => render(3));
  assert.equal(dialogTop(), "300px", "new content grows the dialog downward instead of re-centring it");

  await act(async () => { window.dispatchEvent(new window.Event("resize")); });
  assert.equal(dialogTop(), "300px", "a resize event without a size change keeps the top");

  await act(async () => { window.happyDOM.setViewport({ width: 1280, height: 720 }); });
  assert.equal(dialogTop(), "280px", "a real viewport change re-centres once");

  await act(async () => root.unmount());
  host.remove();
  console.log("✓ FormDialog keeps a stable top while its content changes");
}

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
