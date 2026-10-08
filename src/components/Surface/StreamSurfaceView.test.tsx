// Run: npx tsx src/components/Surface/StreamSurfaceView.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { BROWSER_SURFACE_CHROME, BROWSER_SURFACE_TITLE } from "./browserChrome";
import { createFakeStreamTransport } from "./fakeStreamTransport.testdata";
import type { FrameDecoder } from "./frameFeed";
import { StreamSurfaceView } from "./StreamSurfaceView";
import { capsuleModel, grabSendArgs, screenshotSavedNotice, surfaceLabel, type GrabSelection, type SurfaceState } from "./surfaceChromeModel";
import { resizeArgs, SURFACE_METHOD, type ScreenshotResult } from "./surfaceStreamTransport";
import { flushAsync, getByRole, queryByRole, render, resizeStages, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("StreamSurfaceView");
const decode: FrameDecoder = async () => ({ width: 800, height: 600, image: {} as CanvasImageSource, close: () => undefined });
const state: SurfaceState = { owner: "Jev", title: "Login", url: "https://a.test/", activity: "typing", profile: "clean", profiles: [] };
const label = surfaceLabel(state.owner, BROWSER_SURFACE_TITLE);

async function mount(saved: ScreenshotResult[] = []) {
  const fake = createFakeStreamTransport();
  const view = await render(
    <StreamSurfaceView
      transport={fake.transport}
      chrome={BROWSER_SURFACE_CHROME}
      surfaceTitle={BROWSER_SURFACE_TITLE}
      initialState={{ owner: state.owner }}
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

suite.test("pushed state drives the label and capsule; Take control enables navigation", async () => {
  const { fake, region, unmount } = await mount();
  await resizeStages(640, 480);
  await act(async () => fake.emitState(state));
  assert.equal(getByRole(region(), "status").textContent, capsuleModel(state).text);
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

void suite.run();
