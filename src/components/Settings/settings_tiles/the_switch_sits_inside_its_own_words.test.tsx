// Run: npm run test:unit -- settings_tiles
//
// THE DEFECT. A switch parked at the far edge of a wide row reads as belonging
// to nothing: the words that name it are outside the control entirely, and the
// control is physically larger than they are.
//
// The claim asserted here is structural rather than cosmetic — the switch and
// its label are ONE control, and the switch comes first inside it — so it is
// stated against the rendered tree rather than against a class name, and it is
// self-contained so it can be run against the tree before the fix.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../../lib/bridge";
import { NotificationSurfaces } from "../NotificationSurfaces";
import type { AppConfig } from "../../../lib/types";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function setupDom(): void {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLInputElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ]) {
    Object.defineProperty(globalThis, key, {
      value: window[key as keyof Window],
      configurable: true,
    });
  }
}

setupDom();

/** The surface a build before this change already drew, so the assertion below
 *  is about its shape rather than about a control that had to be added first. */
const SHIPPED_SURFACE = "Desktop notifications";

/** Every surface the section answers for once the three are gathered. */
const SURFACES = ["Show the bell", SHIPPED_SURFACE, "Notification sounds"] as const;

function config(): AppConfig {
  return {
    notifications: { desktop: true, sound: true, show_bell: true },
  } as unknown as AppConfig;
}

async function mount() {
  __setInvokeForTest(async <T,>(command: string) =>
    (command === "notify_permission_state" ? "granted" : null) as T,
  );
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(<NotificationSurfaces config={config()} onUpdate={() => {}} />),
  );
  for (let turn = 0; turn < 4; turn++) await act(async () => {});
  return {
    host,
    async done() {
      await act(async () => root.unmount());
      host.remove();
      __setInvokeForTest(null);
    },
  };
}

function control(host: Element, label: string): HTMLButtonElement {
  const found = host.querySelector(
    `button[role="switch"][aria-label="${label}"]`,
  ) as HTMLButtonElement | null;
  assert(found, `"${label}" is an accessible switch`);
  return found;
}

/** The switch and the words it governs are the same control, and the switch is
 *  the first thing inside it. A row whose control sits after the words, outside
 *  them, is the shape this replaces. */
function assertSwitchThenLabel(host: Element, label: string): void {
  const tile = control(host, label);
  assert(
    tile.textContent?.includes(label),
    `the words "${label}" must live inside the control they name, got "${tile.textContent}"`,
  );
  const parts = Array.from(tile.children);
  assert(parts.length >= 2, `"${label}" renders a switch and its words, got ${parts.length} parts`);
  const [first, second] = parts;
  assert(
    first.getAttribute("aria-hidden") === "true" && (first.textContent ?? "") === "",
    `"${label}" leads with the drawn switch, not with text`,
  );
  assert(
    (second.textContent ?? "").startsWith(label),
    `the words follow the switch, got "${second.textContent}"`,
  );
  const wording = Array.from(host.querySelectorAll("span")).filter(
    (node) => (node.textContent ?? "") === label,
  );
  assert(wording.length > 0, `"${label}" is written somewhere on the surface`);
  assert(
    wording.every((node) => tile.contains(node)),
    `every rendering of "${label}" is inside the control it names, never beside it`,
  );
}

async function theSwitchAndItsWordsAreOneControl() {
  const view = await mount();
  assertSwitchThenLabel(view.host, SHIPPED_SURFACE);
  await view.done();
}

async function everySurfaceIsDrawnThatWay() {
  const view = await mount();
  for (const label of SURFACES) assertSwitchThenLabel(view.host, label);
  assert(
    view.host.querySelectorAll('button[role="switch"]').length === SURFACES.length,
    "the section answers for exactly the surfaces it names",
  );
  await view.done();
}

const cases: string[] = [];
async function check(name: string, run: () => Promise<void>): Promise<void> {
  await run();
  cases.push(name);
  console.log(`  ok ${cases.length} - ${name}`);
}

async function main() {
  await check(
    "settings_tiles: a surface switch and its words are one control, switch first",
    theSwitchAndItsWordsAreOneControl,
  );
  await check("settings_tiles: every surface is drawn that way", everySurfaceIsDrawnThatWay);
  console.log(`PASS settings_tiles — the switch sits inside its own words — ${cases.length}/${cases.length} cases`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
