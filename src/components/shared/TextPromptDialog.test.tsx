// Run: npx tsx src/components/shared/TextPromptDialog.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { Window } from "happy-dom";
import { accessibleName, getByRole } from "../../test/semanticRoles";

const window = new Window({ url: "http://localhost/" });
const define = (name: string, value: unknown) => Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
define("IS_REACT_ACT_ENVIRONMENT", true);
define("window", window);
define("document", window.document);
define("navigator", window.navigator);
for (const name of ["HTMLElement", "HTMLInputElement", "Element", "Node", "Event", "KeyboardEvent", "MouseEvent", "FocusEvent", "InputEvent"]) {
  define(name, (window as unknown as Record<string, unknown>)[name]);
}

// react-dom decides at load whether `input` events exist, so it must load after the DOM globals.
const modules = Promise.all([import("react-dom/client"), import("./TextPromptDialog")]);

function dialog(): HTMLElement {
  const element = document.body.querySelector<HTMLElement>("[role='dialog']");
  if (!element) throw new Error("no dialog");
  return element;
}

function textbox(name: string): HTMLInputElement {
  const label = [...document.body.querySelectorAll("label")].find((candidate) => candidate.querySelector("span")?.textContent === name);
  const input = label ? document.getElementById(label.getAttribute("for") ?? "") : null;
  if (!(input instanceof window.HTMLInputElement)) throw new Error(`no textbox named ${name}`);
  return input as unknown as HTMLInputElement;
}

function button(name: string): HTMLButtonElement {
  return getByRole(dialog(), "button", name) as HTMLButtonElement;
}

async function type(input: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new window.Event("input", { bubbles: true }) as unknown as Event);
  });
}

async function mount(validate?: (value: string) => string | null) {
  const submitted: string[] = [];
  let cancels = 0;
  const [{ createRoot }, { TextPromptDialog }] = await modules;
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(
    <TextPromptDialog
      title="New browser profile"
      label="Profile name"
      confirmLabel="Create"
      validate={validate}
      onSubmit={(value) => submitted.push(value)}
      onCancel={() => { cancels += 1; }}
    />,
  ));
  return {
    submitted,
    cancels: () => cancels,
    async unmount() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

async function main() {
  {
    const view = await mount();
    assert.equal(accessibleName(dialog()), "New browser profile", "the dialog is named by its title");
    assert.equal(dialog().getAttribute("aria-modal"), "true");
    assert.equal(document.activeElement, textbox("Profile name"), "the field takes focus");
    assert.ok(button("Create").disabled, "an empty value cannot be submitted");
    await type(textbox("Profile name"), "  Work  ");
    await act(async () => button("Create").click());
    assert.deepEqual(view.submitted, ["Work"], "submit fires once with the trimmed value");
    await view.unmount();
  }
  {
    const view = await mount((value) => (/^[\w-]+$/.test(value) ? null : "Use letters, digits, - or _"));
    await type(textbox("Profile name"), "bad name");
    assert.ok(dialog().querySelector("[role='alert']"), "an invalid value shows an alert");
    assert.equal(textbox("Profile name").getAttribute("aria-invalid"), "true");
    assert.ok(button("Create").disabled);
    await act(async () => { dialog().dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }) as unknown as Event); });
    assert.deepEqual(view.submitted, [], "Enter on an invalid value submits nothing");
    await act(async () => button("Cancel").click());
    assert.equal(view.cancels(), 1, "Cancel fires once");
    await view.unmount();
  }
  {
    const view = await mount();
    let bubbled = 0;
    document.addEventListener("keydown", () => { bubbled += 1; });
    await act(async () => { textbox("Profile name").dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }) as unknown as Event); });
    assert.equal(view.cancels(), 1, "Escape cancels");
    assert.equal(bubbled, 0, "keys stay inside the dialog");
    await view.unmount();
  }
  console.log("✓ TextPromptDialog tests green");
}

void main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
