import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { MeshCanvas } from "./MeshCanvas";
import type { PeerStatus } from "./useMeshData";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const peers = ["ubuntu", "green", "air"].map((name, index) => ({
  instance_id: `peer-${name}`, name, ip: `100.64.0.${index + 2}`, online: name !== "air", pending: false,
})) as unknown as PeerStatus[];

let root: Root | null = null;
let host: HTMLDivElement;

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  host.remove();
});

async function mount(width: number, touch: boolean) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<MeshCanvas thisMachine={{ name: "BLACKSTAR", ip: "192.168.1.196" }} peers={peers}
    onPeerClick={() => {}} recentPushes={[]} width={width} height={700} touch={touch} />));
}

const list = () => host.querySelector("[data-testid=mesh-inventory-list]");
const toggle = () => host.querySelector<HTMLButtonElement>("[data-testid=mesh-inventory-view-toggle]");
const layer = () => host.querySelector<HTMLElement>("[data-peer-card][data-machine-index]")?.parentElement ?? null;
const scale = () => Number(/scale\(([^)]+)\)/.exec(layer()!.style.transform)?.[1]);
const tap = (testId: string) => act(async () => host.querySelector<HTMLButtonElement>(`[data-testid=${testId}]`)!.click());

function touchEvent(type: string, points: Array<[number, number]>) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "touches", { value: points.map(([clientX, clientY]) => ({ clientX, clientY })) });
  return event;
}

it("a touch phone opens on the stacked list with every machine, and Graph switches to the canvas", async () => {
  await mount(390, true);
  expect(list()).not.toBeNull();
  expect(list()!.querySelectorAll("[role=listitem]")).toHaveLength(1 + 1 + peers.length);
  expect(layer()).toBeNull();
  await act(async () => toggle()!.click());
  expect(list()).toBeNull();
  expect(layer()).not.toBeNull();
  await act(async () => toggle()!.click());
  expect(list()).not.toBeNull();
});

it("on touch, +/- visibly change the graph's scale and Fit all brings it back", async () => {
  await mount(390, true);
  await act(async () => toggle()!.click());
  const fitted = scale();
  await tap("canvas-zoom-in");
  expect(scale()).toBeCloseTo(fitted * 1.25);
  await tap("canvas-zoom-out");
  expect(scale()).toBeCloseTo(fitted * 1.25 * 0.75);
  await tap("canvas-zoom-fit");
  expect(scale()).toBeCloseTo(fitted);
});

it("pinch zooms the graph and is kept from the page; a one-finger touch stays a tap and pans", async () => {
  await mount(390, true);
  await act(async () => toggle()!.click());
  const canvas = layer()!.parentElement!;
  const before = scale();
  const start = touchEvent("touchstart", [[100, 300], [200, 300]]);
  const pinch = touchEvent("touchmove", [[50, 300], [250, 300]]);
  await act(async () => { canvas.dispatchEvent(start); canvas.dispatchEvent(pinch); });
  expect(start.defaultPrevented).toBe(true);
  expect(pinch.defaultPrevented).toBe(true);
  expect(scale()).toBeCloseTo(before * 2);
  const x = () => Number(/translate\(([-\d.e]+)px/.exec(layer()!.style.transform)?.[1]);
  const pinchedX = x();
  const tap = touchEvent("touchstart", [[100, 300]]);
  await act(async () => {
    canvas.dispatchEvent(touchEvent("touchend", []));
    canvas.dispatchEvent(tap);
    canvas.dispatchEvent(touchEvent("touchmove", [[140, 300]]));
  });
  expect(tap.defaultPrevented).toBe(false);
  expect(x()).toBeCloseTo(pinchedX + 40);
  expect(scale()).toBeCloseTo(before * 2);
});

it("desktop is unchanged: graph only, no list toggle, and the fine zoom step", async () => {
  await mount(1200, false);
  expect(list()).toBeNull();
  expect(toggle()).toBeNull();
  const fitted = scale();
  await tap("canvas-zoom-in");
  expect(scale()).toBeCloseTo(fitted * 1.06);
});
