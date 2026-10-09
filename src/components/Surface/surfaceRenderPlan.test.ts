// Run: npx tsx src/components/Surface/surfaceRenderPlan.test.ts

import assert from "node:assert/strict";
import { BROWSER_SURFACE_CHROME } from "./browserChrome";
import {
  findDeclaredSurface,
  parseChromeSpec,
  parseDeclaredSurfaces,
  surfaceRenderPlan,
  type DeclaredSurface,
} from "./surfaceRenderPlan";

const listBody = {
  declared: [
    { plugin_id: "dash", surface_id: "board", title: "Board", renderer: "view", agent_bound: true, restore: "relaunch" },
    { plugin_id: "browser", surface_id: "browser", title: "Browser", renderer: "stream", runtime: "browser", agent_bound: true, restore: "placeholder", chrome: { toolbar: [], screenshot: true } },
    { plugin_id: 7, surface_id: "broken" },
  ],
  open: [],
};
const declared = parseDeclaredSurfaces(listBody);
assert.deepEqual(declared, [
  { pluginId: "dash", surfaceId: "board", renderer: "view", runtime: null, chrome: undefined },
  { pluginId: "browser", surfaceId: "browser", renderer: "stream", runtime: "browser", chrome: { toolbar: [], screenshot: true } },
], "rows without ids are skipped");
assert.deepEqual(parseDeclaredSurfaces({ open: [] }), []);
assert.deepEqual(parseDeclaredSurfaces(null), []);
assert.equal(findDeclaredSurface(declared, "browser", "browser")?.runtime, "browser");
assert.equal(findDeclaredSurface(declared, "browser", "board"), null, "plugin and surface ids must both match");

const row = (overrides: Partial<DeclaredSurface>): DeclaredSurface => ({ pluginId: "p", surfaceId: "s", renderer: "view", runtime: null, chrome: undefined, ...overrides });
const ready = (overrides: Partial<DeclaredSurface>) => ({ status: "ready" as const, declared: row(overrides) });

assert.deepEqual(surfaceRenderPlan({ status: "loading" }, true), { kind: "pending" });
assert.deepEqual(surfaceRenderPlan(ready({ renderer: "view" }), true), { kind: "view" });
assert.deepEqual(surfaceRenderPlan({ status: "ready", declared: null }, true), { kind: "unavailable" }, "an undeclared surface never falls back to the plugin view");
assert.deepEqual(surfaceRenderPlan({ status: "failed", error: "HTTP 500" }, true), { kind: "unavailable", error: "HTTP 500" }, "an unreadable declaration carries its error");
assert.deepEqual(surfaceRenderPlan({ status: "failed", error: "HTTP 500" }, false), { kind: "unavailable", error: "HTTP 500" }, "a hidden tab is unavailable too");
assert.deepEqual(
  surfaceRenderPlan(ready({ renderer: "stream", runtime: "browser" }), true),
  { kind: "stream", runtime: "browser", chrome: BROWSER_SURFACE_CHROME, live: true },
  "a stream without chrome falls back to the browser chrome",
);
assert.deepEqual(
  surfaceRenderPlan(ready({ renderer: "stream", runtime: "browser", chrome: { toolbar: [], snip: true } }), false),
  { kind: "stream", runtime: "browser", chrome: { toolbar: [], snip: true }, live: false },
  "declared chrome wins and a hidden tab holds no live stream",
);
assert.deepEqual(surfaceRenderPlan(ready({ renderer: "stream", runtime: "browser", chrome: "nope" }), true).kind, "stream");
assert.equal(surfaceRenderPlan(ready({ renderer: "stream" }), true).kind, "unsupported", "a stream without a runtime is refused");
assert.equal(surfaceRenderPlan(ready({ renderer: "canvas" }), true).kind, "unsupported", "an unknown renderer is refused");

assert.equal(parseChromeSpec(undefined), null);
assert.equal(parseChromeSpec([]), null);
assert.deepEqual(parseChromeSpec({ takeControl: true }), { toolbar: [], takeControl: true }, "a missing toolbar is empty");
assert.deepEqual(
  parseChromeSpec({
    toolbar: [
      { id: "back", kind: "button", label: "Back", icon: "chevron-left", when: "interactive", invoke: { method: "nav", args: { action: "back" } } },
      { id: "address", kind: "input", label: "Address", source: "url", when: "sometimes", invoke: { method: "nav", valueArg: "target", args: [1] } },
      { id: "bad", kind: "slider", label: "Bad" },
      { id: "nolabel", kind: "button" },
      "junk",
    ],
    pointAndAsk: true,
    screenshot: "yes",
  }),
  {
    toolbar: [
      { id: "back", kind: "button", label: "Back", icon: "chevron-left", when: "interactive", invoke: { method: "nav", args: { action: "back" } } },
      { id: "address", kind: "input", label: "Address", source: "url", invoke: { method: "nav", valueArg: "target" } },
    ],
    pointAndAsk: true,
  },
  "valid items survive, malformed items and fields are dropped",
);
assert.deepEqual(parseChromeSpec(BROWSER_SURFACE_CHROME), BROWSER_SURFACE_CHROME, "the built-in chrome round-trips");

assert.deepEqual(parseChromeSpec({ screenshot: true, openExternal: true }), { toolbar: [], snip: true, openExternal: true }, "a legacy screenshot flag declares the snip camera");
assert.deepEqual(parseChromeSpec({ screenshot: true, snip: false }), { toolbar: [], snip: false }, "an explicit snip flag wins");

console.log("✓ surfaceRenderPlan tests green");
