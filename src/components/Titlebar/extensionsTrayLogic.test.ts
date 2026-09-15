// Run: npx tsx src/components/Titlebar/extensionsTrayLogic.test.ts

import { Window } from "happy-dom";
import {
  selectExtension,
  SELECT_EXTENSION_EVENT,
} from "./extensionsTrayLogic";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function setupDom() {
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
}
setupDom();

test("selectExtension dispatches the deep-link event and opens", () => {
  const events: Array<string | null> = [];
  let opened = 0;
  const handler = (e: Event) => events.push((e as CustomEvent<{ id: string | null }>).detail.id);
  window.addEventListener(SELECT_EXTENSION_EVENT, handler);

  selectExtension("bitwarden", () => { opened += 1; });
  selectExtension(null, () => { opened += 1; });

  window.removeEventListener(SELECT_EXTENSION_EVENT, handler);
  assert(events.length === 2 && events[0] === "bitwarden" && events[1] === null, "should dispatch id then null");
  assert(opened === 2, "onOpen must fire each time");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(err);
  }
}
console.log(`extensionsTrayLogic: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
