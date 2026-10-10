// Run: npx tsx src/components/Surface/StreamCanvas.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { createFrameFeed, type FrameDecoder } from "./frameFeed";
import { StreamCanvas, type StreamCanvasProps } from "./StreamCanvas";
import { canvasToPage, clickInput, layoutStream, viewportRequest, type PagePoint, type StreamInput, type StreamViewport } from "./streamModel";
import { liveViewLabel, type PopoverAnchor } from "./surfaceChromeModel";
import { flushAsync, getByRole, queryByRole, render, resizeStages, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("StreamCanvas");
const FRAME = { width: 800, height: 600 };
const META = { deviceWidth: 1280, deviceHeight: 960, offsetTop: 0, pageScaleFactor: 1 };
const decode: FrameDecoder = async () => ({ ...FRAME, image: {} as CanvasImageSource, close: () => undefined });

interface Recorded {
  viewports: StreamViewport[];
  inputs: StreamInput[];
  picks: Array<[PagePoint, PopoverAnchor]>;
  hovers: PagePoint[];
  blocked: number;
  exits: number;
  cancels: number;
}

async function renderCanvas(overrides: Partial<StreamCanvasProps> = {}) {
  const feed = createFrameFeed();
  const recorded: Recorded = { viewports: [], inputs: [], picks: [], hovers: [], blocked: 0, exits: 0, cancels: 0 };
  const props: StreamCanvasProps = {
    label: "Jev's Browser",
    pageTitle: "Login",
    interactive: false,
    picking: false,
    frames: feed,
    decode,
    onViewport: (viewport) => recorded.viewports.push(viewport),
    onInput: (input) => recorded.inputs.push(input),
    onPick: (point, anchor) => recorded.picks.push([point, anchor]),
    onHover: (point) => recorded.hovers.push(point),
    onBlocked: () => { recorded.blocked += 1; },
    onExitInteraction: () => { recorded.exits += 1; },
    onCancelPick: () => { recorded.cancels += 1; },
    ...overrides,
  };
  const view = await render(<StreamCanvas {...props} />);
  await resizeStages(400, 300);
  feed.publish({ data: "", metadata: META });
  await flushAsync();
  return { ...view, recorded };
}

const expectedPoint = (clientX: number, clientY: number) =>
  canvasToPage(clientX, clientY, { left: 0, top: 0, width: 400, height: 300 }, layoutStream(FRAME, { width: 400, height: 300 }, 1).drawCss, META);

suite.test("watch-only canvas is a labelled image and clicks only report blocked", async () => {
  const { host, recorded, unmount } = await renderCanvas();
  const canvas = getByRole(host, "img", liveViewLabel("Jev's Browser", "Login"));
  assert.equal(queryByRole(host, "application"), null);
  await act(async () => canvas.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 200, clientY: 150 })));
  await act(async () => { canvas.dispatchEvent(new KeyboardEvent("keydown", { key: "a", code: "KeyA", bubbles: true })); });
  assert.equal(recorded.blocked, 1);
  assert.deepEqual(recorded.inputs, []);
  await unmount();
});

suite.test("the stage size is reported once per distinct viewport", async () => {
  const { recorded, unmount } = await renderCanvas();
  await resizeStages(400, 300);
  assert.deepEqual(recorded.viewports, [viewportRequest({ width: 400, height: 300 }, window.devicePixelRatio)]);
  await resizeStages(500, 300);
  assert.equal(recorded.viewports.length, 2);
  await unmount();
});

suite.test("collapsing the stage reports hidden once and the same size is reported again", async () => {
  let hidden = 0;
  const { recorded, unmount } = await renderCanvas({ onHidden: () => { hidden += 1; } });
  await resizeStages(0, 0);
  await resizeStages(0, 0);
  assert.equal(hidden, 1, "only the visible-to-hidden edge reports");
  await resizeStages(400, 300);
  const viewport = viewportRequest({ width: 400, height: 300 }, window.devicePixelRatio);
  assert.deepEqual(recorded.viewports, [viewport, viewport]);
  await unmount();
});

suite.test("interactive clicks, keys and wheel are forwarded as mapped input", async () => {
  const { host, recorded, unmount } = await renderCanvas({ interactive: true });
  const canvas = getByRole(host, "application", liveViewLabel("Jev's Browser", "Login"));
  await act(async () => canvas.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 200, clientY: 150, detail: 1 })));
  await act(async () => { canvas.dispatchEvent(new KeyboardEvent("keydown", { key: "a", code: "KeyA", bubbles: true })); });
  const wheel = new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: 40 });
  Object.defineProperties(wheel, { clientX: { value: 200 }, clientY: { value: 150 } });
  await act(async () => { canvas.dispatchEvent(wheel); });
  assert.ok(wheel.defaultPrevented, "an interactive wheel does not scroll Domios");
  const point = expectedPoint(200, 150)!;
  assert.deepEqual(recorded.inputs, [
    clickInput(point, 1),
    { kind: "text", text: "a" },
    { kind: "wheel", x: point.x, y: point.y, deltaX: 0, deltaY: 40 },
  ]);
  await act(async () => { canvas.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", shiftKey: true, bubbles: true })); });
  assert.equal(recorded.exits, 1);
  await unmount();
});

suite.test("picking reports the page point once and Escape cancels", async () => {
  const { host, recorded, unmount } = await renderCanvas({ picking: true });
  const canvas = getByRole(host, "img");
  await act(async () => canvas.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientX: 100, clientY: 100 })));
  await act(async () => canvas.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 100, clientY: 100 })));
  assert.deepEqual(recorded.hovers, [expectedPoint(100, 100)]);
  assert.deepEqual(recorded.picks, [[expectedPoint(100, 100), { x: 100, y: 100, width: 400, height: 300 }]]);
  assert.equal(recorded.blocked, 0, "picking never reports blocked");
  await act(async () => { canvas.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true })); });
  assert.equal(recorded.cancels, 1);
  await unmount();
});

void suite.run();
