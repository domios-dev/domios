// Run: npx tsx src/components/Surface/SurfaceToolbar.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { BROWSER_SURFACE_CHROME } from "./browserChrome";
import { CHROME_ITEM, resolveToolbar, type ResolvedToolbarItem, type SurfaceState } from "./surfaceChromeModel";
import { SurfaceToolbar, type SurfaceToolbarProps } from "./SurfaceToolbar";
import { getByRole, render, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("SurfaceToolbar");
const state: SurfaceState = { owner: "Jev", url: "https://a.test/", profile: "clean", profiles: ["Work"] };

interface Calls {
  button: string[];
  toggle: string[];
  submit: Array<[string, string]>;
  select: Array<[string, string]>;
}

async function renderToolbar(items: ResolvedToolbarItem[]) {
  const calls: Calls = { button: [], toggle: [], submit: [], select: [] };
  const props: SurfaceToolbarProps = {
    label: "Jev's Browser",
    items,
    onButton: (id) => calls.button.push(id),
    onToggle: (id) => calls.toggle.push(id),
    onSubmit: (id, value) => calls.submit.push([id, value]),
    onSelect: (id, value) => calls.select.push([id, value]),
  };
  const view = await render(<SurfaceToolbar {...props} />);
  return { ...view, calls, toolbar: getByRole(view.host, "toolbar", "Jev's Browser controls") };
}

function setNativeValue(element: HTMLInputElement | HTMLSelectElement, value: string) {
  for (let prototype = Object.getPrototypeOf(element); prototype; prototype = Object.getPrototypeOf(prototype)) {
    const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
    if (setter) return setter.call(element, value);
  }
}

suite.test("every resolved item renders with its role, name and enabled state", async () => {
  const items = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: false, picking: false, state });
  const { toolbar, unmount } = await renderToolbar(items);
  for (const item of items) {
    const role = item.kind === "input" ? "textbox" : item.kind === "select" ? "combobox" : "button";
    const control = getByRole(toolbar, role, item.label) as HTMLButtonElement;
    assert.equal(control.disabled, item.disabled, `${item.id} disabled state follows the model`);
    if (item.kind === "toggle") assert.equal(control.getAttribute("aria-pressed"), String(item.pressed), `${item.id} pressed state`);
  }
  await unmount();
});

suite.test("buttons and toggles fire their handler once", async () => {
  const items = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: true, picking: false, state });
  const { toolbar, calls, unmount } = await renderToolbar(items);
  await act(async () => getByRole(toolbar, "button", "Back").click());
  await act(async () => getByRole(toolbar, "button", "Open in browser").click());
  await act(async () => getByRole(toolbar, "button", "Take control").click());
  await act(async () => {
    getByRole(toolbar, "button", "Snip and ask").dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));
  });
  assert.deepEqual(calls.button, ["back", CHROME_ITEM.openExternal]);
  assert.deepEqual(calls.toggle, [CHROME_ITEM.takeControl, CHROME_ITEM.snip], "Shift has no second meaning on the camera");
  assert.equal(getByRole(toolbar, "button", "Snip and ask").getAttribute("title"), "Snip and ask");
  await unmount();
});

suite.test("address submits on Enter and the profile select reports its value", async () => {
  const items = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: true, picking: false, state });
  const { toolbar, calls, unmount } = await renderToolbar(items);
  const address = getByRole(toolbar, "textbox", "Address") as HTMLInputElement;
  assert.equal(address.value, "https://a.test/", "address shows the model value");
  address.focus();
  setNativeValue(address, "example.com");
  await act(async () => { address.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); });
  assert.deepEqual(calls.submit, [["address", "example.com"]]);

  const profile = getByRole(toolbar, "combobox", "Browser profile") as HTMLSelectElement;
  await act(async () => {
    setNativeValue(profile, "Work");
    profile.dispatchEvent(new Event("change", { bubbles: true }));
  });
  assert.deepEqual(calls.select, [["profile", "Work"]]);
  await unmount();
});

suite.test("Open in browser sits right after the address field", async () => {
  const items = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: false, picking: false, state });
  const { toolbar, unmount } = await renderToolbar(items);
  const address = getByRole(toolbar, "textbox", "Address");
  const open = getByRole(toolbar, "button", "Open in browser");
  assert.equal(address.nextElementSibling, open);
  assert.equal(open.getAttribute("title"), "Open in browser");
  assert.ok(open.classList.contains("ct-surface-icon-btn"), "the quiet ghost icon style of the other toolbar icons");
  assert.ok(open.querySelector("svg"), "icon only");
  await unmount();
});

void suite.run();
