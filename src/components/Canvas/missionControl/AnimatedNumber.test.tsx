import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { AnimatedNumber, CHANGED_VALUE_CLASS } from "./AnimatedNumber";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let reducedMotion = false;
let frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
let root: Root | null = null;
let host: HTMLElement | null = null;

function installHarness() {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (media: string) => ({
      matches: reducedMotion,
      media,
      addEventListener() {},
      removeEventListener() {},
    }),
  });
  const request = (callback: FrameRequestCallback) => {
    const id = ++nextFrame;
    frames.set(id, callback);
    return id;
  };
  const cancel = (id: number) => frames.delete(id);
  for (const target of [globalThis, window]) {
    Object.defineProperty(target, "requestAnimationFrame", { configurable: true, value: request });
    Object.defineProperty(target, "cancelAnimationFrame", { configurable: true, value: cancel });
  }
}

async function show(value: number | null) {
  await act(async () => root!.render(<AnimatedNumber value={value} testId="figure" />));
  return host!.querySelector<HTMLElement>('[data-testid="figure"]')!;
}

beforeEach(() => {
  reducedMotion = false;
  frames = new Map();
  nextFrame = 0;
  installHarness();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("AnimatedNumber changed-value highlight", () => {
  it("does not highlight the first reading or a load from no value", async () => {
    expect((await show(null)).dataset.pulse).toBeUndefined();
    const first = await show(4);
    expect(first.dataset.pulse).toBeUndefined();
    expect(first.classList.contains(CHANGED_VALUE_CLASS)).toBe(false);
  });

  it("highlights a changed value exactly once and keeps an unchanged re-render still", async () => {
    const before = await show(1);
    const same = await show(1);
    expect(same).toBe(before);
    expect(same.dataset.pulse).toBeUndefined();

    const changed = await show(2);
    expect(changed).not.toBe(before);
    expect(changed.dataset.pulse).toBe("1");
    expect(changed.classList.contains(CHANGED_VALUE_CLASS)).toBe(true);

    const rerendered = await show(2);
    expect(rerendered).toBe(changed);
    expect(rerendered.dataset.pulse).toBe("1");

    const next = await show(3);
    expect(next).not.toBe(changed);
    expect(next.dataset.pulse).toBe("2");
  });

  it("suppresses the highlight and the count-up under reduced motion", async () => {
    reducedMotion = true;
    await show(1);
    const changed = await show(5);
    expect(changed.dataset.pulse).toBeUndefined();
    expect(changed.classList.contains(CHANGED_VALUE_CLASS)).toBe(false);
    expect(changed.textContent).toBe("5");
    expect(frames.size).toBe(0);
  });
});
