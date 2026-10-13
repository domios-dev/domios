// Run: npx tsx src/components/Surface/StreamSurfaceView.test.tsx

import "./surfaceDomGlobals.testdata";
import { resetCaptureOwnershipForTest } from "../Terminal/voice/localRecordingCaptureModel";
import assert from "node:assert/strict";
import { act } from "react";
import { BROWSER_SURFACE_CHROME, BROWSER_SURFACE_TITLE } from "./browserChrome";
import { surfaceControlStore } from "../../lib/surfaceControlStore";
import { createFakeStreamTransport, type FakeStreamTransport } from "./fakeStreamTransport.testdata";
import type { FrameDecoder } from "./frameFeed";
import { StreamSurfaceView } from "./StreamSurfaceView";
import { SNIP_IMAGE_LABEL } from "./PointAndAsk";
import {
  blockedNotice,
  grabSendArgs,
  needsHumanNotice,
  NEW_PROFILE_OPTION,
  profileSwitchedNotice,
  SNIP_DIALOG_LABEL,
  surfaceLabel,
  type GrabSelection,
  type SnipCapture,
  type SurfaceState,
} from "./surfaceChromeModel";
import { resizeArgs, SURFACE_METHOD, type StreamFrame } from "./surfaceStreamTransport";
import { PREVIEW_PAUSED_TEXT } from "./SurfacePreviewPaused";
import { changeValue, flushAsync, getByRole, queryByRole, render, resizeStages, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("StreamSurfaceView");
const decode: FrameDecoder = async () => ({ width: 800, height: 600, image: {} as CanvasImageSource, close: () => undefined });
const state: SurfaceState = { owner: "Jev", title: "Login", url: "https://a.test/", activity: "typing", profile: "clean", profiles: [] };
const label = surfaceLabel(state.owner, BROWSER_SURFACE_TITLE);

interface MountOptions {
  ownerName?: string;
  surfaceTabId?: string;
  opened?: string[];
}

async function mount({ ownerName, surfaceTabId, opened = [] }: MountOptions = {}) {
  const fake = createFakeStreamTransport();
  const view = await render(
    <StreamSurfaceView
      transport={fake.transport}
      chrome={BROWSER_SURFACE_CHROME}
      surfaceTitle={BROWSER_SURFACE_TITLE}
      initialState={{ owner: state.owner }}
      ownerName={ownerName}
      surfaceTabId={surfaceTabId}
      decode={decode}
      openExternal={(url) => opened.push(url)}
    />,
  );
  return { ...view, fake, region: () => getByRole(view.host, "region", label) };
}

const SURFACE_TAB = "surface-1";

/** Owner annotations must leave the page to the agent: no input and no Take control on the way. */
function watchControl(fake: FakeStreamTransport) {
  const flips: boolean[] = [];
  const unsubscribe = surfaceControlStore.subscribe(() => flips.push(surfaceControlStore.get(SURFACE_TAB)));
  return {
    assertUntouched(region: HTMLElement) {
      unsubscribe();
      assert.deepEqual(fake.callsTo(SURFACE_METHOD.input), [], "no page input");
      assert.deepEqual(flips, [], "no control transition");
      assert.equal(surfaceControlStore.get(SURFACE_TAB), false, "the agent keeps control");
      assert.equal(queryByRole(region, "status", blockedNotice(state.owner)), null, "no Take control prompt");
    },
  };
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

const capture: SnipCapture = { id: "s1", width: 640, height: 480, thumbnail: "data:image/jpeg;base64,AA==" };
const pointer = (type: string, clientX: number, clientY: number) =>
  new PointerEvent(type, { bubbles: true, cancelable: true, clientX, clientY, button: 0, pointerId: 1 });
const snipOverlay = (scope: ParentNode) => scope.querySelector<HTMLElement>("[data-testid='surface-snip']");
const escape = () => new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
const snipFrame = (scope: ParentNode) => scope.querySelector<HTMLElement>("[data-testid='surface-snip-frame']");

function assertFrameAtSelection(scope: ParentNode) {
  const frame = snipFrame(scope);
  assert.ok(frame, "the snipped area stays outlined on the live page");
  assert.deepEqual(
    [frame.style.left, frame.style.top, frame.style.width, frame.style.height],
    ["100px", "75px", "100px", "75px"],
    "the frame sits where the area was dragged, in stage px",
  );
}

function assertNoLargeImage(scope: ParentNode) {
  const images = [...scope.querySelectorAll("img")];
  assert.ok(
    images.every((image) => image.dataset.testid === "attachment-chip-thumb"),
    "the only image in the surface is the chip thumbnail",
  );
}

async function startSnip(options: MountOptions = {}) {
  const view = await mount({ surfaceTabId: SURFACE_TAB, ...options });
  view.fake.respond(SURFACE_METHOD.snipCapture, () => capture);
  view.fake.respond(SURFACE_METHOD.snipSend, () => ({ markdown: "", delivered: true }));
  await resizeStages(400, 300);
  await act(async () => view.fake.emitState(state));
  await act(async () => view.fake.emitFrame({ data: "", metadata: { deviceWidth: 1280, deviceHeight: 960, scrollOffsetX: 0, scrollOffsetY: 640 } }));
  await flushAsync();
  const control = watchControl(view.fake);
  await act(async () => getByRole(view.region(), "button", "Snip and ask").click());
  return { ...view, control };
}

async function dragArea(scope: ParentNode) {
  const overlay = snipOverlay(scope);
  assert.ok(overlay, "the camera opens the area selection over the stream");
  await act(async () => { overlay.dispatchEvent(pointer("pointerdown", 100, 75)); });
  await act(async () => { overlay.dispatchEvent(pointer("pointermove", 200, 150)); });
  assert.equal(overlay.querySelector("[data-testid='surface-snip-size']")?.textContent, "320 × 240", "the live size is in page CSS px");
  await act(async () => { overlay.dispatchEvent(pointer("pointerup", 200, 150)); });
  await flushAsync();
}

async function ask(host: HTMLElement, text: string) {
  const dialog = getByRole(host, "dialog", SNIP_DIALOG_LABEL);
  const editor = getByRole(dialog, "textbox", "Your question") as HTMLTextAreaElement;
  await changeValue(editor as unknown as HTMLInputElement, text, "input");
  await act(async () => { editor.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
  await flushAsync();
}

suite.test("snip and ask works beside the agent: drag, capture in page coords, one image send", async () => {
  const { fake, region, host, control, unmount } = await startSnip();
  await dragArea(region());
  assert.equal(snipOverlay(region()), null, "release finishes the selection");
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.snipCapture).map((call) => call.args), [{ x: 320, y: 880, w: 320, h: 240 }], "canvas rect maps to the scrolled page");
  assertFrameAtSelection(region());
  assertNoLargeImage(host);
  const dialog = getByRole(host, "dialog", SNIP_DIALOG_LABEL);
  const chip = dialog.querySelector<HTMLElement>("[data-testid='attachment-chip']");
  assert.equal(chip?.textContent, SNIP_IMAGE_LABEL, "the snip sits in the input as a screenshot chip");
  assert.equal(chip?.querySelector("img")?.getAttribute("src"), capture.thumbnail);
  assert.ok(dialog.textContent?.includes("Area 320×240 at (320, 880) · Login · https://a.test/"));
  await ask(host, "why is this cut off?");
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.snipSend).map((call) => call.args), [{ id: "s1", comment: "why is this cut off?", image: true }]);
  assert.equal(fake.callsTo(SURFACE_METHOD.snipCancel).length, 0, "a sent capture is not discarded");
  assert.equal(queryByRole(host, "dialog"), null);
  assert.equal(snipFrame(region()), null, "sending clears the frame with the composer");
  control.assertUntouched(region());
  await unmount();
});

suite.test("removing the screenshot chip sends the question with only the area context", async () => {
  const { fake, region, host, control, unmount } = await startSnip();
  await dragArea(region());
  const dialog = getByRole(host, "dialog", SNIP_DIALOG_LABEL);
  await act(async () => getByRole(dialog, "button", `Remove ${SNIP_IMAGE_LABEL}`).click());
  assert.equal(dialog.querySelector("[data-testid='attachment-chip']"), null, "x removes the chip");
  assert.ok(dialog.textContent?.includes("Area 320×240 at (320, 880)"), "the context stays");
  await ask(host, "what is this?");
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.snipSend).map((call) => call.args), [{ id: "s1", comment: "what is this?", image: false }]);
  control.assertUntouched(region());
  await unmount();
});

suite.test("Esc cancels the selection and the composer without sending anything", async () => {
  const { fake, region, host, control, unmount } = await startSnip();
  await act(async () => { snipOverlay(region())!.dispatchEvent(escape()); });
  assert.equal(snipOverlay(region()), null, "Esc leaves the selection");
  assert.deepEqual(fake.calls.filter((call) => call.method.startsWith("snip.")), [], "nothing captured");
  await act(async () => getByRole(region(), "button", "Snip and ask").click());
  await dragArea(region());
  assertFrameAtSelection(region());
  const editor = getByRole(getByRole(host, "dialog", SNIP_DIALOG_LABEL), "textbox", "Your question");
  await changeValue(editor as unknown as HTMLInputElement, "draft", "input");
  await act(async () => { editor.dispatchEvent(escape()); });
  await flushAsync();
  assert.equal(queryByRole(host, "dialog"), null);
  assert.equal(snipFrame(region()), null, "Esc clears the frame with the composer");
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.snipSend), [], "nothing sent");
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.snipCancel).map((call) => call.args), [{ id: "s1" }], "the capture is discarded");
  control.assertUntouched(region());
  await unmount();
});

suite.test("a snip cancelled while its capture is in flight discards the late capture and opens nothing", async () => {
  const { fake, region, host, unmount } = await startSnip();
  let finish: (value: SnipCapture) => void = () => undefined;
  fake.respond(SURFACE_METHOD.snipCapture, () => new Promise<SnipCapture>((resolve) => { finish = resolve; }));
  await dragArea(region());
  assertFrameAtSelection(region());
  await act(async () => getByRole(region(), "button", "Pick element").click());
  assert.equal(snipFrame(region()), null, "switching to Pick clears the pending frame");
  await act(async () => finish(capture));
  await flushAsync();
  assert.equal(queryByRole(host, "dialog"), null, "the late capture opens no composer");
  assert.equal(snipFrame(region()), null);
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.snipCancel).map((call) => call.args), [{ id: "s1" }], "the late capture is discarded");
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.snipSend), []);
  await unmount();
});

suite.test("a click without a drag selects nothing and keeps the selection open", async () => {
  const { fake, region, unmount } = await startSnip();
  const overlay = snipOverlay(region())!;
  await act(async () => { overlay.dispatchEvent(pointer("pointerdown", 50, 50)); });
  await act(async () => { overlay.dispatchEvent(pointer("pointerup", 51, 50)); });
  await flushAsync();
  assert.equal(fake.callsTo(SURFACE_METHOD.snipCapture).length, 0);
  assert.ok(snipOverlay(region()));
  await unmount();
});

suite.test("Open in browser opens exactly the current URL once and is off for about:blank", async () => {
  const opened: string[] = [];
  const { fake, region, unmount } = await mount({ opened });
  await resizeStages(640, 480);
  await act(async () => fake.emitState(state));
  const open = getByRole(region(), "button", "Open in browser") as HTMLButtonElement;
  assert.ok(!open.disabled, "usable without Take control");
  await act(async () => open.click());
  assert.deepEqual(opened, ["https://a.test/"]);
  await act(async () => fake.emitState({ ...state, url: "about:blank" }));
  assert.ok(open.disabled);
  await act(async () => open.click());
  assert.deepEqual(opened, ["https://a.test/"], "a disabled button opens nothing");
  assert.equal(fake.calls.filter((call) => call.method !== SURFACE_METHOD.resize).length, 0, "opening never touches the agent's page");
  await unmount();
});

suite.test("point and ask works beside the agent: selects, sends the annotation and closes", async () => {
  const selection: GrabSelection = {
    id: "g1",
    label: "button \"Sign in\"",
    target: { tagName: "button", selector: "form > button", accessibility: { role: "button", name: "Sign in" } },
  };
  const { fake, region, host, unmount } = await mount({ surfaceTabId: SURFACE_TAB });
  fake.respond(SURFACE_METHOD.grabSelect, () => selection);
  await resizeStages(400, 300);
  await act(async () => fake.emitState(state));
  await act(async () => fake.emitFrame({ data: "", metadata: { deviceWidth: 1280, deviceHeight: 960 } }));
  await flushAsync();
  const control = watchControl(fake);
  await act(async () => getByRole(region(), "button", "Pick element").click());
  await act(async () => getByRole(region(), "img").dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 200, clientY: 150 })));
  await flushAsync();
  assert.equal(fake.callsTo(SURFACE_METHOD.grabSelect).length, 1);
  const dialog = getByRole(host, "dialog", "Ask about Button «Sign in»");
  const editor = getByRole(dialog, "textbox", "Your question") as HTMLTextAreaElement;
  await changeValue(editor as unknown as HTMLInputElement, "rename this", "input");
  await act(async () => { editor.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
  await flushAsync();
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.grabSend).map((call) => call.args), [{ id: "g1", comment: "rename this", intent: "question" }]);
  assert.deepEqual(grabSendArgs(selection, "rename this"), { id: "g1", comment: "rename this", intent: "question" });
  assert.equal(fake.callsTo(SURFACE_METHOD.grabCancel).length, 1, "sending ends the pick on the host");
  assert.equal(queryByRole(host, "dialog"), null);
  control.assertUntouched(region());
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

suite.test("hiding and re-showing the stage reconnects at the same size", async () => {
  const { fake, unmount } = await mount();
  const viewport = { w: 640, h: 480, dpr: window.devicePixelRatio };
  await resizeStages(640, 480);
  await act(async () => fake.emitOpen());
  await resizeStages(0, 0);
  await resizeStages(640, 480);
  assert.deepEqual(fake.callsTo(SURFACE_METHOD.resize).map((call) => call.args), [resizeArgs(viewport)]);
  assert.equal(fake.connections.length, 2, "show explicitly reconnects the stream");
  assert.equal(fake.disconnects(), 1);
  await unmount();
});

suite.test("explicit tab show resumes once and keeps the current canvas without a new frame", async () => {
  const fake = createFakeStreamTransport();
  const props = { transport: fake.transport, chrome: BROWSER_SURFACE_CHROME, surfaceTitle: BROWSER_SURFACE_TITLE, initialState: state, decode };
  const { host, root, unmount } = await render(<StreamSurfaceView {...props} visible />);
  await resizeStages(640, 480);
  await act(async () => fake.emitFrame({ data: "", metadata: { deviceWidth: 640, deviceHeight: 480 } }));
  const canvas = getByRole(host, "img");
  for (let cycle = 0; cycle < 10; cycle += 1) {
    await act(async () => root.render(<StreamSurfaceView {...props} visible={false} />));
    await act(async () => root.render(<StreamSurfaceView {...props} visible />));
    assert.equal(getByRole(host, "img"), canvas, "the drawn canvas survives show");
  }
  assert.equal(fake.connections.length, 11);
  assert.equal(fake.disconnects(), 10);
  await unmount();
});

const frameAt = (deviceWidth: number, deviceHeight: number): StreamFrame => ({ data: "", metadata: { deviceWidth, deviceHeight } });

suite.test("a viewport mismatch heals once until a matching frame arrives", async () => {
  const { fake, unmount } = await mount();
  const resizes = () => fake.callsTo(SURFACE_METHOD.resize).map((call) => call.args);
  await resizeStages(640, 480);
  await act(async () => fake.emitFrame(frameAt(640, 480)));
  assert.deepEqual(resizes(), [], "a matching frame heals nothing");
  await act(async () => fake.emitFrame(frameAt(1280, 960)));
  assert.deepEqual(resizes(), [resizeArgs({ w: 640, h: 480, dpr: window.devicePixelRatio })]);
  await act(async () => fake.emitFrame(frameAt(1280, 960)));
  assert.equal(resizes().length, 1, "duplicate mismatch cannot loop");
  await act(async () => fake.emitFrame(frameAt(640, 480)));
  await act(async () => fake.emitFrame(frameAt(1280, 960)));
  assert.equal(resizes().length, 2, "a new mismatch after recovery heals again");
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
  const { fake, host, unmount } = await mount({ ownerName: "Mira" });
  await resizeStages(640, 480);
  await act(async () => fake.emitState(state));
  assert.ok(getByRole(host, "region", surfaceLabel("Mira", BROWSER_SURFACE_TITLE)));
  await unmount();
});

// The composer mic keeps a 30s warm-stream timer that would hold the process open.
void suite.run().then(resetCaptureOwnershipForTest);
