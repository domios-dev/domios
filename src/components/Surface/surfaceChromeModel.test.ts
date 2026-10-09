// Run: npx tsx src/components/Surface/surfaceChromeModel.test.ts

import assert from "node:assert/strict";
import { BROWSER_SURFACE_CHROME } from "./browserChrome";
import {
  addressSubmission,
  askPopoverPosition,
  canOpenExternally,
  canvasHint,
  CHROME_ITEM,
  ELEMENT_NAME_MAX,
  elementCard,
  grabSendArgs,
  navigationFailureNotice,
  isNeedsHuman,
  liveViewLabel,
  NEEDS_HUMAN_KINDS,
  needsHumanNotice,
  NEW_PROFILE_OPTION,
  PROFILE_NAME_MAX,
  profileOptions,
  profileSwitchedNotice,
  resolveToolbar,
  selectChoice,
  SNIP_HINT,
  snipCaptureArgs,
  snipContext,
  snipPlaceholder,
  snipSendArgs,
  snipSizeLabel,
  surfaceLabel,
  toolbarInvocation,
  validateProfileName,
  withOwner,
  withTranscripts,
  type GrabTarget,
  type SurfaceState,
  type ToolbarItemSpec,
} from "./surfaceChromeModel";

const labelCases: Array<[string, string, string]> = [
  ["Jev", "Browser", "Jev's Browser"],
  ["  Jev ", "Browser", "Jev's Browser"],
  ["", "Browser", "Browser"],
];
for (const [owner, title, expected] of labelCases) assert.equal(surfaceLabel(owner, title), expected);

assert.equal(liveViewLabel("Jev's Browser", "Login"), "Live view of Jev's Browser, Login");
assert.equal(liveViewLabel("Jev's Browser", "  "), "Live view of Jev's Browser");
assert.equal(liveViewLabel("Jev's Browser"), "Live view of Jev's Browser");

assert.deepEqual([...NEEDS_HUMAN_KINDS], ["captcha", "consent", "login", "dialog", "file_chooser"]);
for (const kind of NEEDS_HUMAN_KINDS) assert.ok(isNeedsHuman(kind), kind);
for (const value of ["none", null, undefined, "", "CAPTCHA"]) assert.ok(!isNeedsHuman(value), String(value));

const base: SurfaceState = { owner: "Jev" };
const noticeCases: Array<[string, SurfaceState, string | null]> = [
  ["no need is no notice", base, null],
  ["needs none is not a need", { ...base, needsHuman: "none", activity: "typing" }, null],
  ["null is not a need", { ...base, needsHuman: null }, null],
  ["a cookie dialog names the owner", { ...base, needsHuman: "consent" }, "Jev needs you: cookie dialog"],
  ["a captcha reads as a bot check", { ...base, activity: "navigating", needsHuman: "captcha" }, "Jev needs you: bot check"],
  ["a blank owner still names someone", { owner: "  ", needsHuman: "login" }, "The agent needs you: sign-in"],
];
for (const [name, state, expected] of noticeCases) assert.equal(needsHumanNotice(state), expected, name);
const needsTexts = NEEDS_HUMAN_KINDS.map((kind) => needsHumanNotice({ ...base, needsHuman: kind }));
assert.equal(new Set(needsTexts).size, NEEDS_HUMAN_KINDS.length, "every needs-human kind has its own notice text");
for (const text of needsTexts) {
  assert.ok(text?.startsWith("Jev needs you: "), String(text));
  assert.ok(!/\bYou\b/.test(text ?? ""), "the notice never names the viewer as the owner");
}

const page: SurfaceState = { ...base, url: "https://a.test/" };
const ids = (state: SurfaceState, interactive: boolean, picking = false) =>
  resolveToolbar(BROWSER_SURFACE_CHROME, { interactive, picking, state }).map((item) => [item.id, item.disabled] as const);
assert.deepEqual(ids(page, false), [
  ["back", true], ["forward", true], ["reload", true], ["address", true], [CHROME_ITEM.openExternal, false],
  [CHROME_ITEM.pick, false], [CHROME_ITEM.snip, false], ["profile", false], [CHROME_ITEM.takeControl, false],
], "watch-only disables page input only: open, pick and snip stay usable beside the agent, in the owner's order");
assert.deepEqual(ids(page, true).filter(([, disabled]) => disabled), [], "interaction enables every item");
assert.deepEqual(ids(base, true).filter(([, disabled]) => disabled), [[CHROME_ITEM.openExternal, true]], "no page url, nothing to open");

const toggles = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: false, picking: true, snipping: false, state: page }).filter((item) => item.kind === "toggle");
assert.deepEqual(toggles.map((item) => [item.id, item.kind === "toggle" && item.pressed]), [[CHROME_ITEM.pick, true], [CHROME_ITEM.snip, false], [CHROME_ITEM.takeControl, false]]);
const snipping = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: false, picking: false, snipping: true, state: page }).find((item) => item.id === CHROME_ITEM.snip);
assert.deepEqual(snipping, { id: CHROME_ITEM.snip, kind: "toggle", label: "Snip and ask", icon: "camera", pressed: true, disabled: false });
const external = resolveToolbar({ toolbar: [{ id: "x", kind: "button", label: "X" }], openExternal: true }, { interactive: false, picking: false, state: page });
assert.deepEqual(external.map((item) => item.id), ["x", CHROME_ITEM.openExternal], "without an address the open button ends the navigation group");

const externalCases: Array<[string | undefined, boolean]> = [
  ["https://a.test/x?y=1", true], ["http://localhost:5173/", true], ["file:///C:/site/index.html", true],
  ["about:blank", false], ["", false], [undefined, false], ["chrome://settings", false], ["javascript:alert(1)", false], ["data:text/html,hi", false], ["not a url", false],
];
for (const [url, expected] of externalCases) assert.equal(canOpenExternally(url), expected, String(url));

const area = { x: 312, y: 96, width: 420, height: 180 };
assert.equal(snipContext(area, { title: " Cart\n review ", url: "https://shop.test/cart" }), "Area 420×180 at (312, 96) · Cart review · https://shop.test/cart");
assert.equal(snipContext(area, {}), "Area 420×180 at (312, 96)", "missing title and url drop out");
assert.equal(snipContext(area, { title: "t".repeat(80) }).length, "Area 420×180 at (312, 96) · ".length + 60, "a long title is cut");
assert.equal(snipSizeLabel(area), "420 × 180");
assert.deepEqual(snipCaptureArgs(area), { x: 312, y: 96, w: 420, h: 180 });
assert.deepEqual(snipSendArgs({ id: "s1", width: 840, height: 360, thumbnail: null }, "  why?  ", false), { id: "s1", comment: "why?", image: false });
assert.equal(snipPlaceholder("Jev"), "Ask Jev about this area");
assert.equal(canvasHint(false, false, true), SNIP_HINT);
assert.equal(SNIP_HINT, "Drag to select an area · Esc to cancel");

const address = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: true, picking: false, state: { ...base, url: "https://a.test/" } }).find((item) => item.id === "address");
assert.equal(address?.kind === "input" && address.value, "https://a.test/", "address shows the page url");

const busy = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: true, picking: false, state: page, busy: new Set(["profile", CHROME_ITEM.snip]) });
assert.deepEqual(busy.filter((item) => item.disabled).map((item) => item.id), [CHROME_ITEM.snip, "profile"], "busy items are disabled");

const bare = resolveToolbar({ toolbar: [{ id: "x", kind: "select", label: "X", source: "unknown" }] }, { interactive: true, picking: false, state: base });
assert.deepEqual(bare, [{ id: "x", kind: "select", label: "X", options: [], value: "", disabled: true }], "unknown select sources render disabled and chrome flags off add nothing");

assert.deepEqual(profileOptions({ ...base, profile: "Andrey", profiles: ["Andrey", "Work"] }), {
  options: [
    { value: "clean", label: "Clean profile" },
    { value: "Andrey", label: "Profile: Andrey" },
    { value: "Work", label: "Profile: Work" },
    { value: NEW_PROFILE_OPTION, label: "+ New profile…" },
  ],
  value: "Andrey",
  locked: false,
});
assert.equal(profileOptions({ ...base, profile: "Gone", profiles: [] }).value, "clean", "an unknown current profile falls back to clean");
assert.deepEqual(profileOptions({ ...base, profile: "attached" }).locked, true, "attach mode locks the profile");
const attached = resolveToolbar(BROWSER_SURFACE_CHROME, { interactive: true, picking: false, state: { ...base, profile: "attached" } }).find((item) => item.id === "profile");
assert.equal(attached?.disabled, true);

const declared = (id: string) => BROWSER_SURFACE_CHROME.toolbar.find((item) => item.id === id) as ToolbarItemSpec;
const invocationCases: Array<[string, ToolbarItemSpec, string | undefined, { method: string; args: Record<string, unknown> }]> = [
  ["back", declared("back"), undefined, { method: "nav", args: { action: "back" } }],
  ["address carries its value", declared("address"), "example.com", { method: "nav", args: { action: "goto", target: "example.com" } }],
  ["profile carries its value", declared("profile"), "Work", { method: "profiles.use", args: { name: "Work" } }],
  ["undeclared invoke is a generic action", { id: "zoom", kind: "button", label: "Zoom" }, undefined, { method: "action", args: { id: "zoom" } }],
  ["generic action carries a value", { id: "q", kind: "input", label: "Q" }, "v", { method: "action", args: { id: "q", value: "v" } }],
];
for (const [name, item, value, expected] of invocationCases) assert.deepEqual(toolbarInvocation(item, value), expected, name);
assert.deepEqual(declared("back").invoke?.args, { action: "back" }, "invocation does not mutate the declared args");

assert.equal(addressSubmission("  a.test "), "a.test");
assert.equal(addressSubmission("   "), null);

const friendly = (target: GrabTarget, label = "") => elementCard({ id: "g1", label, target });
const friendlyCases: Array<[string, GrabTarget, string, string]> = [
  ["a link uses its accessible name", { tagName: "a", accessibility: { role: "link", name: "snake." } }, "link", "Link «snake.»"],
  ["a button uses its accessible name", { tagName: "button", text: "Play now", accessibility: { role: "button", name: "Play" } }, "button", "Button «Play»"],
  ["an image uses its alt name", { tagName: "img", accessibility: { role: "image", name: "logo" } }, "image", "Image «logo»"],
  ["the legacy img role is an image", { tagName: "img", accessibility: { role: "img", name: "logo" } }, "image", "Image «logo»"],
  ["a heading names its level-free role", { tagName: "h2", accessibility: { role: "heading", name: "Pricing" } }, "heading", "Heading «Pricing»"],
  ["a text field is a field", { tagName: "input", accessibility: { role: "textbox", name: "Email" } }, "input", "Field «Email»"],
  ["a landmark keeps its own role", { tagName: "nav", accessibility: { role: "navigation", name: "Main" } }, "text", "Navigation «Main»"],
  ["a generic node falls back to its visible text", { tagName: "span", text: "s", accessibility: { role: "generic", name: "" } }, "text", "Text «s»"],
  ["visible text keeps the tag's kind", { tagName: "a", text: "  Read   more " }, "link", "Link «Read more»"],
  ["no name and no text is the tag", { tagName: "DIV", accessibility: { role: "generic" } }, "text", "div"],
];
for (const [name, target, kind, expected] of friendlyCases) {
  const card = friendly(target);
  assert.equal(card.name, expected, name);
  assert.equal(card.kind, kind, name);
}
const long = friendly({ tagName: "button", accessibility: { role: "button", name: "Continue to the very long checkout page" } }).name;
assert.ok(long.length <= ELEMENT_NAME_MAX, `${long} fits the chip`);
assert.ok(long.startsWith("Button «Continue") && long.endsWith("…»"), long);
assert.ok(friendly({ tagName: "custom-very-long-element-name-here" }).name.endsWith("…"));
assert.deepEqual(friendly({ react: "LoginForm", selector: "form > button" }, "button Sign in"), { kind: "text", name: "button Sign in", path: "LoginForm · form > button" }, "a legacy card keeps the host label");
assert.equal(friendly({ tagName: "main", selector: "html > body > main" }).path, "html > body > main");
assert.deepEqual(grabSendArgs({ id: "g7", label: "x", target: {} }, "  why?  "), { id: "g7", comment: "why?", intent: "question" }, "the backend contract defaults to a question");
assert.deepEqual(grabSendArgs({ id: "g7", label: "x", target: {} }, "why?", "bug"), { id: "g7", comment: "why?", intent: "bug" });
assert.equal(withTranscripts("  look here ", ["  make it blue ", ""]), "look here\n\nmake it blue");
assert.equal(withTranscripts("", ["only voice"]), "only voice");

const popoverCases: Array<[string, { x: number; y: number; width: number; height: number }, { left: number; top: number }]> = [
  ["opens beside the click", { x: 100, y: 50, width: 1000, height: 800 }, { left: 112, top: 62 }],
  ["stays inside the right and bottom edges", { x: 990, y: 790, width: 1000, height: 800 }, { left: 652, top: 592 }],
  ["keeps a margin in a tiny stage", { x: 0, y: 0, width: 100, height: 100 }, { left: 8, top: 8 }],
];
for (const [name, anchor, expected] of popoverCases) assert.deepEqual(askPopoverPosition(anchor), expected, name);

const profileSelect = BROWSER_SURFACE_CHROME.toolbar.find((item) => item.id === "profile") as ToolbarItemSpec;
assert.deepEqual(selectChoice(profileSelect, NEW_PROFILE_OPTION), { kind: "new-profile" }, "the new-profile entry opens the dialog");
assert.deepEqual(selectChoice(profileSelect, "clean"), { kind: "value", value: "" }, "clean is sent as the empty name");
assert.deepEqual(selectChoice(profileSelect, "Work"), { kind: "value", value: "Work" });
assert.deepEqual(selectChoice({ source: "url" }, NEW_PROFILE_OPTION), { kind: "value", value: NEW_PROFILE_OPTION }, "only profile selects know the sentinel");

const nameCases: Array<[string, boolean]> = [
  ["Work", true],
  ["  work-2_b ", true],
  ["a".repeat(PROFILE_NAME_MAX), true],
  ["a".repeat(PROFILE_NAME_MAX + 1), false],
  ["", false],
  ["two words", false],
  ["../etc", false],
  ["Clean", false],
  ["attached", false],
];
for (const [name, valid] of nameCases) assert.equal(validateProfileName(name) === null, valid, `profile name ${JSON.stringify(name)}`);

assert.equal(profileSwitchedNotice("Work"), "Using profile Work");
assert.equal(profileSwitchedNotice("clean"), profileSwitchedNotice(""), "clean and empty read the same");

const pushed: SurfaceState = { owner: "agent-7" };
assert.deepEqual(withOwner(pushed, "Jev"), { owner: "Jev" }, "the owner tab's name wins over the pushed owner");
assert.equal(withOwner(pushed, "  "), pushed, "a blank owner tab name keeps the pushed owner");
assert.equal(withOwner(pushed, null), pushed);

console.log("✓ surfaceChromeModel tests green");

assert.equal(navigationFailureNotice("127.0.0.1:9", new Error("navigation failed: net::ERR_UNSAFE_PORT")), null);
assert.equal(navigationFailureNotice("example.com", new Error("browser is not open")), "Could not open example.com");
assert.equal(navigationFailureNotice(undefined, "boom"), "Navigation failed");
