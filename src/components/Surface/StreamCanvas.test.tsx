// Run: npx tsx src/components/Surface/StreamCanvas.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { createFrameFeed, type FrameDecoder } from "./frameFeed";
import { StreamCanvas, type StreamCanvasProps } from "./StreamCanvas";
import { canvasToPage, clickInput, layoutStream, viewportRequest, type PagePoint, type StreamInput, type StreamViewport } from "./streamModel";
import { liveViewLabel, type PopoverAnchor } from "./surfaceChromeModel";
import { acceptViewportFrame } from "./resizeFrameModel";
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

async function renderCanvas(overrides: Partial<StreamCanvasProps> = {}, resize = true) {
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
  if (resize) await resizeStages(400, 300);
  feed.publish({ data: "", metadata: META });
  await flushAsync();
  return { ...view, recorded, feed };
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
  assert.equal(recorded.inputs.length, 0);
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

suite.test("cropped frames map the page edge after resizing and reject the black letterbox", async () => {
  const { host, feed, recorded, unmount } = await renderCanvas({ interactive: true });
  feed.publish({ data: "", metadata: { deviceWidth: 800, deviceHeight: 600, offsetTop: 100 } });
  await flushAsync();
  await resizeStages(800, 800);
  const canvas = getByRole(host, "application", liveViewLabel("Jev's Browser", "Login"));
  await act(async () => {
    canvas.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 0, clientY: 150 }));
    canvas.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 400, clientY: 149 }));
  });
  assert.deepEqual(recorded.inputs, [clickInput({ x: 0, y: 0 }, 0)]);
  await unmount();
});

for (const dpr of [1.25, 1.5]) {
  suite.test(`first layout at DPR ${dpr} reports the real size before an observer callback`, async () => {
    const previousRect = HTMLElement.prototype.getBoundingClientRect;
    const previousDpr = window.devicePixelRatio;
    HTMLElement.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 640.4, 480.6);
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: dpr });
    try {
      const { recorded, unmount } = await renderCanvas({}, false);
      assert.deepEqual(recorded.viewports, [{ w: 640, h: 481, dpr }]);
      await resizeStages(640.4, 480.6);
      assert.equal(recorded.viewports.length, 1, "the first observer notification does not duplicate the request");
      await unmount();
    } finally {
      HTMLElement.prototype.getBoundingClientRect = previousRect;
      Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: previousDpr });
    }
  });
}

suite.test("DPR notifications measure immediately and rearm for the next monitor", async () => {
  const previousDpr = window.devicePixelRatio;
  const previousMatchMedia = window.matchMedia;
  const media: Array<HTMLElement & { media: string }> = [];
  window.matchMedia = (query) => {
    const next = Object.assign(document.createElement("div"), { media: query });
    media.push(next);
    return next as unknown as MediaQueryList;
  };
  try {
    const { recorded, unmount } = await renderCanvas();
    for (const dpr of [1.25, 1.5]) {
      Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: dpr });
      media.slice(-1)[0]!.dispatchEvent(new Event("change"));
      assert.deepEqual(recorded.viewports.slice(-1)[0], { w: 400, h: 300, dpr });
      assert.equal(media.slice(-1)[0]!.media, `(resolution: ${dpr}dppx)`);
    }
    assert.equal(recorded.viewports.length, 3);
    await unmount();
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 2 });
    media.slice(-1)[0]!.dispatchEvent(new Event("change"));
    assert.equal(recorded.viewports.length, 3, "unmount removes the resolution listener");
  } finally {
    window.matchMedia = previousMatchMedia;
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: previousDpr });
  }
});

suite.test("low-density decoded frames are drawn and replaced by the next frame", async () => {
  const previousContext = HTMLCanvasElement.prototype.getContext;
  const previousDpr = window.devicePixelRatio;
  const drawn: unknown[] = [];
  const image = {} as CanvasImageSource;
  HTMLCanvasElement.prototype.getContext = (() => ({ drawImage: (...args: unknown[]) => drawn.push(args) })) as unknown as typeof previousContext;
  Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 1.5 });
  let width = 200;
  let closed = 0;
  try {
    const { host, feed, recorded, unmount } = await renderCanvas({
      decode: async () => ({ width, height: width * 0.75, image, close: () => { closed += 1; } }),
    });
    const canvas = getByRole(host, "img") as HTMLCanvasElement;
    assert.deepEqual(drawn, [[image, 0, 0, 200, 150, 0, 0, 200, 150]], "the first lower-density bitmap is painted");
    assert.equal(canvas.width, 200);
    width = 600;
    feed.publish({ data: "", metadata: META });
    await flushAsync();
    assert.deepEqual(drawn.slice(-1)[0], [image, 0, 0, 600, 450, 0, 0, 600, 450]);
    assert.equal(canvas.width, 600);
    assert.equal(closed, 1, "only replacement releases the lower-density frame");
    assert.equal(recorded.viewports.length, 1, "frames never trigger resize requests");
    await unmount();
    assert.equal(closed, 2);
  } finally {
    HTMLCanvasElement.prototype.getContext = previousContext;
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: previousDpr });
  }
});

suite.test("resizing retains the native backing store and drops a decode from an obsolete viewport", async () => {
  let viewport: StreamViewport = { w: 400, h: 300, dpr: 1 };
  const feed = createFrameFeed((next) => acceptViewportFrame(next.metadata, viewport));
  let finishDecode: (() => void) | null = null;
  let closed = 0;
  const { host, unmount } = await renderCanvas({
    frames: feed,
    decode: async () => {
      await new Promise<void>((resolve) => { finishDecode = resolve; });
      return { width: 400, height: 300, image: {} as CanvasImageSource, close: () => { closed += 1; } };
    },
  });
  const canvas = getByRole(host, "img") as HTMLCanvasElement;
  feed.publish({ data: "", metadata: { deviceWidth: 400, deviceHeight: 300 } });
  assert.ok(finishDecode);
  (finishDecode as () => void)();
  await flushAsync();
  assert.equal(canvas.width, 400);
  assert.equal(canvas.height, 300);
  feed.publish({ data: "", metadata: { deviceWidth: 400, deviceHeight: 300 } });
  viewport = { w: 500, h: 300, dpr: 1 };
  await resizeStages(500, 300);
  assert.equal(canvas.width, 400, "the old bitmap survives a CSS resize without backing-store allocation");
  assert.equal(canvas.height, 300);
  (finishDecode as unknown as () => void)();
  await flushAsync();
  assert.equal(closed, 1, "only the obsolete decode is released");
  assert.equal(canvas.width, 400);
  await unmount();
  assert.equal(closed, 2, "the retained good frame lives until unmount");
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

suite.test("dragging sends ordered pointer phases without an extra click", async () => {
  const { host, recorded, unmount } = await renderCanvas({ interactive: true });
  const canvas = getByRole(host, "application");
  for (const [type, x] of [["pointerdown", 50], ["pointermove", 150], ["pointerup", 200]] as const) {
    await act(async () => canvas.dispatchEvent(new PointerEvent(type, { bubbles: true, pointerId: 1, button: 0, clientX: x, clientY: 100 })));
  }
  await act(async () => canvas.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 200, clientY: 100, detail: 1 })));
  assert.deepEqual(recorded.inputs.map((input) => input.kind === "pointer" ? input.phase : input.kind), ["down", "move", "up"]);
  assert.deepEqual(recorded.inputs.map((input) => input.kind === "pointer" ? input.buttons : null), [1, 1, 0]);
  await unmount();
});

suite.test("copy and paste shortcuts use the clipboard bridge instead of shell shortcuts", async () => {
  const actions: string[] = [];
  const { host, recorded, unmount } = await renderCanvas({ interactive: true, onCopy: () => actions.push("copy"), onPaste: () => actions.push("paste") });
  const canvas = getByRole(host, "application");
  await act(async () => canvas.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "c", code: "KeyC", ctrlKey: true })));
  await act(async () => canvas.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "v", code: "KeyV", metaKey: true })));
  assert.deepEqual(actions, ["copy", "paste"]);
  assert.deepEqual(recorded.inputs, []);
  await unmount();
});

suite.test("mouse double-click preserves the native click count for word selection", async () => {
  const { host, recorded, unmount } = await renderCanvas({ interactive: true });
  const canvas = getByRole(host, "application");
  await act(async () => canvas.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerType: "mouse", pointerId: 2, button: 0, clientX: 100, clientY: 100 })));
  assert.equal(recorded.inputs.length, 0);
  await act(async () => canvas.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, detail: 2, clientX: 100, clientY: 100 })));
  await act(async () => canvas.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "mouse", pointerId: 2, button: 0, clientX: 100, clientY: 100 })));
  assert.deepEqual(recorded.inputs.map((input) => input.kind === "pointer" ? input.clickCount : 0), [2, 2]);
  await unmount();
});

suite.test("a frame decoded after unmount is closed instead of retained", async () => {
  let complete: ((value: Awaited<ReturnType<FrameDecoder>>) => void) | undefined;
  let closed = 0;
  const { unmount } = await renderCanvas({ decode: () => new Promise((resolve) => { complete = resolve; }) });
  await unmount();
  complete!({ ...FRAME, image: {} as CanvasImageSource, close: () => { closed += 1; } });
  await flushAsync();
  assert.equal(closed, 1);
});

void suite.run();
