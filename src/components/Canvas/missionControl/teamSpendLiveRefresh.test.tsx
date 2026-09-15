import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { __setListenForTest } from "../../../lib/bridge";
import { KPI_RECORDED_EVENT } from "../../../lib/kpiChanges";
import type { ApiConfig } from "../../../lib/types";
import { SpendCard } from "./BoardSpendCard";
import { teamSpendRange, useTeamSpend, type TeamSpend } from "./teamSpend";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const API: ApiConfig = { enabled: true, host: "127.0.0.1", port: 7686, token: "test-token", cors: false };

type ListenHandler = (event: { payload: unknown }) => void;
const listeners = new Map<string, Set<ListenHandler>>();
let frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
let clock = 0;
let root: Root | null = null;
let host: HTMLElement | null = null;

function spend(newInput: number, generated: number): TeamSpend {
  const range = teamSpendRange();
  return {
    team_id: "team-1",
    metric: "tokens",
    units: "tokens",
    scope: "team",
    model: "all",
    from: range.from,
    to: range.to,
    state: "recording",
    not_recording_reason: null,
    plans: [{ plan_id: "pl_1", title: "Plan one", status: "active" }],
    attribution: { team_plan_count: 1, unknown_plan_rows: 0, unassigned_plan_rows: 0, foreign_plan_rows_excluded: 0 },
    daily: [{
      day: range.to,
      team_id: "team-1",
      plan_id: "pl_1",
      plan_title: "Plan one",
      plan_status: "active",
      provider: "claude",
      model: "opus",
      input_tokens: newInput * 2,
      uncached_input_tokens: newInput,
      output_tokens: generated,
      turns: 3,
    }],
    weekly_limit_points: [],
    weekly_limit_days: [],
  };
}

function response(value: TeamSpend): Response {
  return { ok: true, json: async () => value } as Response;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function installHarness() {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (media: string) => ({ matches: false, media, addEventListener() {}, removeEventListener() {} }),
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
  __setListenForTest(async (event, handler) => {
    const set = listeners.get(event) ?? new Set<ListenHandler>();
    listeners.set(event, set);
    set.add(handler as ListenHandler);
    return () => set.delete(handler as ListenHandler);
  });
}

function emitCommit() {
  for (const handler of listeners.get(KPI_RECORDED_EVENT) ?? []) handler({ payload: { plan_ids: ["pl_1"] } });
}

async function flush() {
  await act(async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  });
}

async function drainFrames() {
  await act(async () => {
    const callbacks = [...frames.values()];
    frames.clear();
    clock += 1_000;
    for (const callback of callbacks) callback(clock);
  });
  await flush();
}

function Probe() {
  return <SpendCard teamSpend={useTeamSpend(API, "team-1", "r1")} />;
}

async function render() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Probe />));
  await flush();
  return host;
}

function figure(view: HTMLElement, testId: string): HTMLElement {
  return view.querySelector<HTMLElement>(`[data-testid="${testId}"] > span`)!;
}

beforeEach(() => {
  listeners.clear();
  frames = new Map();
  nextFrame = 0;
  clock = 0;
  installHarness();
  vi.stubGlobal("fetch", vi.fn());
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
  __setListenForTest(null);
  vi.unstubAllGlobals();
});

describe("team spend live refresh", () => {
  it("re-reads once for a burst of KPI commits without blanking and highlights only changed totals", async () => {
    const fetchMock = globalThis.fetch as unknown as ReturnType<typeof vi.fn>;
    const second = deferred<Response>();
    fetchMock.mockResolvedValueOnce(response(spend(100, 50)));
    fetchMock.mockImplementationOnce(() => second.promise);
    const view = await render();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const generatedBefore = figure(view, "mc-board-spend-generated");

    emitCommit();
    emitCommit();
    emitCommit();
    await flush();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    await drainFrames();
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(view.querySelector('[data-testid="mc-board-spend-empty"]')).toBeNull();
    expect(figure(view, "mc-board-spend-new-input").textContent).toBe("100");

    second.resolve(response(spend(200, 50)));
    await flush();
    await drainFrames();
    await drainFrames();
    expect(figure(view, "mc-board-spend-new-input").textContent).toBe("200");
    expect(figure(view, "mc-board-spend-new-input").dataset.pulse).toBe("1");
    expect(figure(view, "mc-board-spend-generated")).toBe(generatedBefore);
    expect(figure(view, "mc-board-spend-generated").dataset.pulse).toBeUndefined();
  });

  it("holds commits during an in-flight read to one trailing read", async () => {
    const fetchMock = globalThis.fetch as unknown as ReturnType<typeof vi.fn>;
    const second = deferred<Response>();
    fetchMock.mockResolvedValueOnce(response(spend(100, 50)));
    fetchMock.mockImplementationOnce(() => second.promise);
    fetchMock.mockResolvedValue(response(spend(300, 50)));
    await render();

    emitCommit();
    await drainFrames();
    expect(fetchMock).toHaveBeenCalledTimes(2);

    emitCommit();
    emitCommit();
    await drainFrames();
    expect(fetchMock).toHaveBeenCalledTimes(2);

    second.resolve(response(spend(200, 50)));
    await flush();
    await drainFrames();
    expect(fetchMock).toHaveBeenCalledTimes(3);
    await drainFrames();
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("stops listening when unmounted", async () => {
    const fetchMock = globalThis.fetch as unknown as ReturnType<typeof vi.fn>;
    fetchMock.mockResolvedValue(response(spend(100, 50)));
    await render();
    expect(listeners.get(KPI_RECORDED_EVENT)?.size).toBe(1);
    await act(async () => root!.unmount());
    root = null;
    expect(listeners.get(KPI_RECORDED_EVENT)?.size).toBe(0);
  });
});
