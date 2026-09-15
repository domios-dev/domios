import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AnalyticsGroupBy, AnalyticsQuery, AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import { formatClock } from "../../../../src/components/Canvas/missionControl/missionControlBoardModel";
import { KPI_CHANGE_NOTICE } from "../../../../src/lib/kpiChanges";
import type { QueryOutcome } from "../../src/plugin";
import { Dashboard } from "./Dashboard";
import { snapshotFor, totals } from "./model/fixtures.testdata";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLElement | null = null;
const calls: AnalyticsQuery[] = [];
let reading: Partial<AnalyticsSnapshot> = {};
let hold: Promise<void> | null = null;
let frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
let clock = 0;

function installHarness() {
  window.matchMedia = ((media: string) =>
    ({ matches: false, media, addEventListener() {}, removeEventListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia;
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
  (window as unknown as { ct: unknown }).ct = {
    invoke: vi.fn(async (_method: string, args: { query: AnalyticsQuery }) => {
      calls.push(args.query);
      if (hold) await hold;
      const outcome: QueryOutcome = { kind: "snapshot", snapshot: snapshotFor(args.query.group_by as AnalyticsGroupBy, reading) };
      return outcome;
    }),
    close: vi.fn(),
  };
}

/** One unfiltered pass reads every grouping once, so workspace reads count passes. */
function passes(): number {
  return calls.filter((query) => query.group_by === "workspace").length;
}

async function settle() {
  for (let i = 0; i < 6; i += 1) await act(async () => {});
}

async function post(data: Record<string, unknown>) {
  await act(async () => {
    window.dispatchEvent(new MessageEvent("message", { data, source: window.parent }));
  });
}

async function drainFrames() {
  await act(async () => {
    const callbacks = [...frames.values()];
    frames.clear();
    clock += 1_000;
    for (const callback of callbacks) callback(clock);
  });
  await settle();
}

async function render() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Dashboard />));
  await settle();
  return host;
}

function figure(view: HTMLElement, key: string): HTMLElement {
  return view.querySelector<HTMLElement>(`[data-testid="analytics-total-${key}-value"] > span`)!;
}

beforeEach(() => {
  calls.length = 0;
  reading = {};
  hold = null;
  frames = new Map();
  nextFrame = 0;
  clock = 0;
  installHarness();
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("analytics freshness", () => {
  it("offers no Refresh control", async () => {
    const view = await render();
    expect(view.querySelector('[data-testid="analytics-refresh"]')).toBeNull();
    expect(view.querySelector('button[aria-label="Refresh"]')).toBeNull();
  });

  it("coalesces a burst of KPI notices into one re-query", async () => {
    await render();
    expect(passes()).toBe(1);
    for (let i = 0; i < 4; i++) await post({ [KPI_CHANGE_NOTICE]: true });
    await settle();
    expect(passes()).toBe(1);
    await drainFrames();
    expect(passes()).toBe(2);
    await drainFrames();
    expect(passes()).toBe(2);
  });

  it("queues one trailing re-query for notices during an in-flight read and keeps figures on screen", async () => {
    const view = await render();
    let release!: () => void;
    hold = new Promise<void>((resolve) => {
      release = resolve;
    });
    await post({ [KPI_CHANGE_NOTICE]: true });
    await drainFrames();
    expect(passes()).toBe(2);

    for (let i = 0; i < 3; i++) await post({ [KPI_CHANGE_NOTICE]: true });
    await drainFrames();
    expect(passes()).toBe(2);
    expect(view.querySelector(".analytics-loading")).toBeNull();
    expect(figure(view, "turns").textContent).toBe("3");

    hold = null;
    await act(async () => release());
    await settle();
    await drainFrames();
    expect(passes()).toBe(3);
    await drainFrames();
    expect(passes()).toBe(3);
  });

  it("ignores notices while hidden and catches up once when shown", async () => {
    await render();
    await post({ __ctActive: true });
    await drainFrames();
    expect(passes()).toBe(1);

    await post({ __ctActive: false });
    for (let i = 0; i < 3; i++) await post({ [KPI_CHANGE_NOTICE]: true });
    await drainFrames();
    expect(passes()).toBe(1);

    await post({ __ctActive: true });
    await drainFrames();
    expect(passes()).toBe(2);

    await post({ __ctActive: true });
    await drainFrames();
    expect(passes()).toBe(2);
  });

  it("advances the as-of caption and highlights only the figures that changed", async () => {
    const view = await render();
    const base = snapshotFor("provider_model");
    expect(view.querySelector('[data-testid="analytics-as-of"]')?.textContent).toBe(`as of ${formatClock(base.as_of * 1000)}`);
    const newInputBefore = figure(view, "new-input");

    const asOf = base.as_of + 37 * 60;
    reading = { as_of: asOf, tokens: { ...base.tokens, totals: totals(300, 30, 1000, 4) } };
    await post({ [KPI_CHANGE_NOTICE]: true });
    await drainFrames();
    await drainFrames();
    await drainFrames();

    expect(view.querySelector('[data-testid="analytics-as-of"]')?.textContent).toBe(`as of ${formatClock(asOf * 1000)}`);
    const turns = figure(view, "turns");
    expect(turns.textContent).toBe("4");
    expect(turns.dataset.pulse).toBe("1");
    expect(figure(view, "new-input")).toBe(newInputBefore);
    expect(figure(view, "new-input").dataset.pulse).toBeUndefined();

    await post({ [KPI_CHANGE_NOTICE]: true });
    await drainFrames();
    expect(figure(view, "turns")).toBe(turns);
    expect(figure(view, "turns").dataset.pulse).toBe("1");
  });
});
