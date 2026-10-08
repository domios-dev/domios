// Run: npx tsx src/components/Surface/StreamSurfaceView.test.tsx

import "./surfaceDomGlobals.testdata";
import assert from "node:assert/strict";
import { act } from "react";
import { BROWSER_SURFACE_CHROME, BROWSER_SURFACE_TITLE } from "./browserChrome";
import { createFakeStreamTransport } from "./fakeStreamTransport.testdata";
import type { FrameDecoder } from "./frameFeed";
import { StreamSurfaceView } from "./StreamSurfaceView";
import {
  grabSendArgs,
  needsHumanNotice,
  NEW_PROFILE_OPTION,
  profileSwitchedNotice,
  screenshotSavedNotice,
  surfaceLabel,
  type GrabSelection,
  type SurfaceState,
} from "./surfaceChromeModel";
import { resizeArgs, SURFACE_METHOD, type ScreenshotResult, type StreamFrame } from "./surfaceStreamTransport";
import { PREVIEW_PAUSED_TEXT } from "./SurfacePreviewPaused";
import { FRAME_HEAL_INTERVAL_MS } from "./useSurfaceStream";
import { changeValue, flushAsync, getByRole, queryByRole, render, resizeStages, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("StreamSurfaceView");
const decode: FrameDecoder = async () => ({ width: 800, height: 600, image: {} as CanvasImageSource, close: () => undefined });
const state: SurfaceState = { owner: "Jev", title: "Login", url: "https://a.test/", activity: "typing", profile: "clean", profiles: [] };
const label = surfaceLabel(state.owner, BROWSER_SURFACE_TITLE);

async function mount(saved: ScreenshotResult[] = [], ownerName?: string) {
  const fake = createFakeStreamTransport();
  const view = await render(
    <StreamSurfaceView
      transport={fake.transport}
      chrome={BROWSER_SURFACE_CHROME}
      surfaceTitle={BROWSER_SURFACE_TITLE}
      initialState={{ owner: state.owner }}
      ownerName={ownerName}
      decode={decode}
      saveScreenshot={(shot) => saved.push(shot)}
    />,
  );
  return { ...view, fake, region: () => getByRole(view.host, "region", label) };
}

suite.test("connects with the first measured viewport, then resizes and re-sends on open", async () => {
  const { fake, unmount } = await mount();
  assert.equal(fake.connections.length, 0, "no stream before the stage has a size");
  await resizeStages(640, 480);
  assert.deepEqual(fake.connections, [{ w: 640, h: 480, dpr: window.devicePixelRatio }]);
  await act(async () => fake.emitOpen());
  await resizeStages(800, 600);
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.resize).map((call) => call.args), [
    resizeArgs({ w: 640, h: 480, dpr: window.devicePixelRatio }),
    resizeArgs({ w: 800, h: 600, dpr: window.devicePixelRatio }),
  ]);
  assert.equal(fake.connections.length, 1, "a resize never reconnects");
  await unmount();
  assert.equal(fake.disconnects(), 1, "unmount disconnects the stream");
});

suite.test("pushed state drives the label; Take control enables navigation", async () => {
  const { fake, region, unmount } = await mount();
  await resizeStages(640, 480);
  await act(async () => fake.emitState(state));
  assert.equal(queryByRole(region(), "status"), null, "an agent with no need shows no notice");
  const back = getByRole(region(), "button", "Back") as HTMLButtonElement;
  assert.ok(back.disabled, "navigation waits for Take control");
  await act(async () => getByRole(region(), "button", "Take control").click());
  assert.ok(!back.disabled);
  await act(async () => back.click());
  assert.deepEqual(fake.callsTo("nav").map((call) => call.args), [{ action: "back" }]);
  await unmount();
});

suite.test("Screenshot invokes once, saves the result and shows the saved notice", async () => {
  const saved: ScreenshotResult[] = [];
  const { fake, region, unmount } = await mount(saved);
  const shot = { base64: "AA==", path: "C:/data/shot-1.png" };
  fake.respond(SURFACE_METHOD.screenshot, () => shot);
  await resizeStages(640, 480);
  await act(async () => getByRole(region(), "button", "Screenshot").click());
  await flushAsync();
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.screenshot).map((call) => call.args), [{ full: false }]);
  assert.deepEqual(saved, [shot]);
  const notice = getByRole(region(), "status", screenshotSavedNotice(shot.path));
  assert.ok(notice);
  await unmount();
});

suite.test("point and ask selects, sends the annotation and closes", async () => {
  const selection: GrabSelection = { id: "g1", label: "button Sign in", target: { selector: "form > button" } };
  const { fake, region, host, unmount } = await mount();
  fake.respond(SURFACE_METHOD.grabSelect, () => selection);
  await resizeStages(400, 300);
  await act(async () => fake.emitState(state));
  await act(async () => fake.emitFrame({ data: "", metadata: { deviceWidth: 1280, deviceHeight: 960 } }));
  await flushAsync();
  await act(async () => getByRole(region(), "button", "Pick element").click());
  await act(async () => getByRole(region(), "img").dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 200, clientY: 150 })));
  await flushAsync();
  assert.equal(fake.callsTo(SURFACE_METHOD.grabSelect).length, 1);
  const dialog = getByRole(host, "dialog", "Ask about element");
  (getByRole(dialog, "textbox", "Your question or request") as HTMLTextAreaElement).value = "rename this";
  await act(async () => getByRole(dialog, "button", "Send").click());
  await flushAsync();
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.grabSend).map((call) => call.args), [grabSendArgs(selection, "rename this", "question")]);
  assert.equal(fake.callsTo(SURFACE_METHOD.grabCancel).length, 1, "sending ends the pick on the host");
  assert.equal(queryByRole(host, "dialog"), null);
  await unmount();
});

suite.test("+ New profile opens a dialog whose name switches the profile", async () => {
  const { fake, region, unmount } = await mount();
  fake.respond("profiles.use", (args) => ({ current: args.name, profiles: [args.name] }));
  await resizeStages(640, 480);
  await act(async () => fake.emitState(state));
  const profile = getByRole(region(), "combobox", "Browser profile") as HTMLSelectElement;
  await changeValue(profile, NEW_PROFILE_OPTION, "change");
  assert.equal(fake.callsTo("profiles.use").length, 0, "choosing + New asks for a name first");
  const dialog = getByRole(document.body, "dialog", "New profile");
  await changeValue(getByRole(dialog, "textbox", "Profile name") as HTMLInputElement, "Work", "input");
  await act(async () => getByRole(dialog, "button", "Create").click());
  await flushAsync();
  assert.deepEqual(fake.callsTo("profiles.use").map((call) => call.args), [{ name: "Work" }]);
  assert.equal(queryByRole(document.body, "dialog", "New profile"), null, "the dialog closes on submit");
  assert.ok(getByRole(region(), "status", profileSwitchedNotice("Work")));
  await unmount();
});

suite.test("cancelling the new-profile dialog switches nothing; Clean sends the empty name", async () => {
  const { fake, region, unmount } = await mount();
  fake.respond("profiles.use", () => ({ current: "clean", profiles: [] }));
  await resizeStages(640, 480);
  await act(async () => fake.emitState({ ...state, profile: "Work", profiles: ["Work"] }));
  const profile = getByRole(region(), "combobox", "Browser profile") as HTMLSelectElement;
  await changeValue(profile, NEW_PROFILE_OPTION, "change");
  await act(async () => getByRole(getByRole(document.body, "dialog", "New profile"), "button", "Cancel").click());
  assert.equal(queryByRole(document.body, "dialog", "New profile"), null);
  await changeValue(profile, "clean", "change");
  await flushAsync();
  assert.deepEqual(fake.callsTo("profiles.use").map((call) => call.args), [{ name: "" }]);
  await unmount();
});

suite.test("a needs-human state shows a dismissible notice that errors override", async () => {
  const { fake, region, unmount } = await mount();
  await resizeStages(640, 480);
  const consent = needsHumanNotice({ ...state, needsHuman: "consent" })!;
  const captcha = needsHumanNotice({ ...state, needsHuman: "captcha" })!;
  const dismiss = async (notice: HTMLElement) => act(async () => getByRole(notice, "button", "Dismiss notice").click());
  await act(async () => fake.emitState({ ...state, needsHuman: "consent" }));
  assert.equal(getByRole(region(), "status", consent).getAttribute("data-tone"), "warn");
  await act(async () => fake.emitNotice({ text: "boom", tone: "error" }));
  assert.equal(queryByRole(region(), "status"), null, "an error notice takes precedence");
  await dismiss(getByRole(region(), "alert"));
  await dismiss(getByRole(region(), "status", consent));
  assert.equal(queryByRole(region(), "status"), null, "dismissing hides the need");
  await act(async () => fake.emitState({ ...state, needsHuman: "consent" }));
  assert.equal(queryByRole(region(), "status"), null, "the same need stays dismissed");
  await act(async () => fake.emitState({ ...state, needsHuman: "captcha" }));
  await dismiss(getByRole(region(), "status", captcha));
  await act(async () => fake.emitState({ ...state, needsHuman: "none" }));
  await act(async () => fake.emitState({ ...state, needsHuman: "captcha" }));
  assert.ok(getByRole(region(), "status", captcha), "a need that went away and came back shows again");
  await unmount();
});

suite.test("hiding and re-showing the stage at the same size re-sends the viewport", async () => {
  const { fake, unmount } = await mount();
  const viewport = { w: 640, h: 480, dpr: window.devicePixelRatio };
  await resizeStages(640, 480);
  await act(async () => fake.emitOpen());
  await resizeStages(0, 0);
  await resizeStages(640, 480);
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.resize).map((call) => call.args), [resizeArgs(viewport), resizeArgs(viewport)]);
  assert.equal(fake.connections.length, 1, "hiding keeps the stream connected");
  assert.equal(fake.disconnects(), 0);
  await unmount();
});

const frameAt = (deviceWidth: number, deviceHeight: number): StreamFrame => ({ data: "", metadata: { deviceWidth, deviceHeight } });

suite.test("a frame at another client's size re-sends the viewport at most once per interval", async () => {
  const { fake, unmount } = await mount();
  const resizes = () => fake.callsTo(SURFACE_METHOD.resize).map((call) => call.args);
  await resizeStages(640, 480);
  await act(async () => fake.emitFrame(frameAt(640, 480)));
  assert.deepEqual(resizes(), [], "a matching frame heals nothing");
  await act(async () => fake.emitFrame(frameAt(1280, 960)));
  assert.deepEqual(resizes(), [resizeArgs({ w: 640, h: 480, dpr: window.devicePixelRatio })]);
  await act(async () => fake.emitFrame(frameAt(1280, 960)));
  assert.equal(resizes().length, 1, "no second heal within the interval");
  const realNow = Date.now;
  Date.now = () => realNow() + FRAME_HEAL_INTERVAL_MS + 1;
  try {
    await act(async () => fake.emitFrame(frameAt(1280, 960)));
  } finally {
    Date.now = realNow;
  }
  assert.equal(resizes().length, 2, "the interval elapsed, so it heals again");
  await flushAsync();
  await unmount();
});

async function withDocument(overrides: { visibilityState?: DocumentVisibilityState; focused?: boolean }, run: () => Promise<void>) {
  const target = document as unknown as Record<string, unknown>;
  if (overrides.visibilityState) Object.defineProperty(document, "visibilityState", { value: overrides.visibilityState, configurable: true });
  if (overrides.focused !== undefined) Object.defineProperty(document, "hasFocus", { value: () => overrides.focused, configurable: true });
  try {
    await run();
  } finally {
    delete target.visibilityState;
    delete target.hasFocus;
  }
}

for (const [name, overrides] of [
  ["a hidden document", { visibilityState: "hidden" }],
  ["an unfocused window", { focused: false }],
] as const) {
  suite.test(`${name} never heals a mismatched frame`, async () => {
    const { fake, unmount } = await mount();
    await resizeStages(640, 480);
    await withDocument(overrides, async () => {
      await act(async () => fake.emitFrame(frameAt(1280, 960)));
    });
    assert.equal(fake.callsTo(SURFACE_METHOD.resize).length, 0);
    await flushAsync();
    await unmount();
  });
}

const pausedNotice = { text: PREVIEW_PAUSED_TEXT, kind: "preview_paused", detail: "no frame: window not rendering (timeout)" } as const;
const byTestId = (scope: ParentNode, id: string) => scope.querySelector<HTMLElement>(`[data-testid='${id}']`);

suite.test("a paused preview shows a quiet inline state, never the notice bar, and clears on the next frame", async () => {
  const { fake, region, unmount } = await mount();
  await resizeStages(640, 480);
  await act(async () => fake.emitNotice(pausedNotice));
  const paused = byTestId(region(), "surface-preview-paused");
  assert.ok(paused, "the canvas shows the paused state");
  assert.equal(paused.textContent?.startsWith(PREVIEW_PAUSED_TEXT), true);
  assert.equal(byTestId(region(), "surface-notice"), null, "a recurring condition is not a notice bar");
  await act(async () => fake.emitNotice({ text: "boom", tone: "error" }));
  assert.ok(byTestId(region(), "surface-notice"), "other notices keep the bar");
  assert.ok(byTestId(region(), "surface-preview-paused"), "and do not clear the paused state");
  await act(async () => fake.emitFrame(frameAt(640, 480)));
  assert.equal(byTestId(region(), "surface-preview-paused"), null, "the next frame clears it");
  await flushAsync();
  await unmount();
});

suite.test("the paused state hides its technical text behind Details", async () => {
  const { fake, region, unmount } = await mount();
  await resizeStages(640, 480);
  await act(async () => fake.emitNotice(pausedNotice));
  const toggle = getByRole(region(), "button", "Details");
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.ok(!region().textContent?.includes(pausedNotice.detail), "the detail starts hidden");
  await act(async () => toggle.click());
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  assert.equal(document.getElementById(toggle.getAttribute("aria-controls")!)?.textContent, pausedNotice.detail);
  await act(async () => toggle.click());
  assert.ok(!region().textContent?.includes(pausedNotice.detail), "Details collapses again");
  await unmount();
});

suite.test("the owner tab's name labels the surface over the pushed owner", async () => {
  const { fake, host, unmount } = await mount([], "Mira");
  await resizeStages(640, 480);
  await act(async () => fake.emitState(state));
  assert.ok(getByRole(host, "region", surfaceLabel("Mira", BROWSER_SURFACE_TITLE)));
  await unmount();
});

void suite.run();
