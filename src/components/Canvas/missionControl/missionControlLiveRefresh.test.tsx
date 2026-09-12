import { act, type ReactElement, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { __setListenForTest } from "../../../lib/bridge";
import type { ApiConfig } from "../../../lib/types";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { MissionControlBoard } from "./MissionControlBoard";
import type { MissionControlState, PlanBoardState } from "./missionControlState";
import { usePlanBoardState, type PlanBoardStateResult } from "./usePlanBoardState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const API: ApiConfig = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: false,
};
const PLAN_ID = "pl_live";
const PLAN: Plan = { ...FIXTURE_PLAN, id: PLAN_ID, updated_at: 7, milestone_kinds: [] };
const NOW = 1_700_000_000_000;

type ListenHandler = (event: { payload: unknown }) => void;
const listeners = new Map<string, Set<ListenHandler>>();
const mediaListeners = new Set<() => void>();
let mediaMatches = false;
let frames = new Map<number, FrameRequestCallback>();
let nextFrameId = 0;

function boardState({ output = 100, turns = 10, bounces = 1, runway = 20, eventCount = 1, tasks = 1 } = {}): PlanBoardState {
  const mission: MissionControlState = {
    held: { held_reviews: [], open_gates: [], oldest_hold_age_ms: null, verdicts_on_held: 0 },
    bounces: { total_attempts: 3, tasks_on_second_attempt: bounces, first_try_done: 2, labelled_bounced: bounces },
    event_log: { recorded_events: eventCount, truncated: false, entries: [] },
    areas: {
      state: "no_facet",
      facet_key: null,
      facet_label: null,
      groups: [],
      untagged_task_count: 0,
      role_fallback: [],
    },
    kpi: {
      state: "recording",
      not_recording_reason: null,
      spend: {
        events: turns,
        bytes: 10_000,
        turns,
        tokens: { input: 200, uncached_input: 20, output, total: output + 200 },
      },
    },
    runway: {
      state: "reporting",
      providers: [{
        provider: "codex",
        label: "Codex",
        captured_at: NOW,
        session_pct: runway,
        session_resets_at: NOW + 60_000,
        weekly_pct: null,
        weekly_resets_at: null,
        stale_reason: null,
      }],
    },
  };
  return {
    plan_id: PLAN_ID,
    task_counts: { pending: 0, in_progress: tasks, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0, total: tasks },
    mission_control: mission,
  };
}

function response(value: PlanBoardState): Response {
  return { ok: true, json: async () => value } as Response;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

function installBrowserHarness() {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: () => ({
      get matches() {
        return mediaMatches;
      },
      addEventListener: (_type: string, listener: () => void) => mediaListeners.add(listener),
      removeEventListener: (_type: string, listener: () => void) => mediaListeners.delete(listener),
      addListener: (listener: () => void) => mediaListeners.add(listener),
      removeListener: (listener: () => void) => mediaListeners.delete(listener),
    }),
  });
  const requestAnimationFrame = (callback: FrameRequestCallback) => {
    const id = ++nextFrameId;
    frames.set(id, callback);
    return id;
  };
  const cancelAnimationFrame = (id: number) => frames.delete(id);
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: requestAnimationFrame });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: cancelAnimationFrame });
  Object.defineProperty(window, "requestAnimationFrame", { configurable: true, value: requestAnimationFrame });
  Object.defineProperty(window, "cancelAnimationFrame", { configurable: true, value: cancelAnimationFrame });
}

function installListenHarness() {
  __setListenForTest(async (event, handler) => {
    const listener = handler as ListenHandler;
    let set = listeners.get(event);
    if (!set) {
      set = new Set();
      listeners.set(event, set);
    }
    set.add(listener);
    return () => set?.delete(listener);
  });
}

function emit(event: string, payload: unknown) {
  for (const handler of listeners.get(event) ?? []) handler({ payload });
}

async function flush() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return {
    host,
    root,
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function text(host: HTMLElement, testId: string): string {
  return host.querySelector(`[data-testid="${testId}"]`)?.textContent?.trim() ?? "";
}

function Probe({ revision, capture }: { revision: number; capture: { current: PlanBoardStateResult | null } }) {
  const result = usePlanBoardState(API, PLAN_ID, revision);
  useEffect(() => {
    capture.current = result;
  }, [capture, result]);
  return (
    <MissionControlBoard
      plan={PLAN}
      state={result.state}
      loading={result.loading}
      error={result.error}
      now={NOW}
    />
  );
}

function advanceFrames(timestamp: number) {
  const callbacks = [...frames.values()];
  frames.clear();
  for (const callback of callbacks) callback(timestamp);
}

beforeEach(() => {
  listeners.clear();
  mediaListeners.clear();
  mediaMatches = false;
  frames = new Map();
  nextFrameId = 0;
  installBrowserHarness();
  installListenHarness();
  vi.stubGlobal("fetch", vi.fn());
});

afterEach(() => {
  __setListenForTest(null);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  listeners.clear();
  mediaListeners.clear();
});

describe("Mission Control live refresh", () => {
  it("mission_control_refreshes_on_relevant_event_without_revision_change", async () => {
    const fetchMock = globalThis.fetch as unknown as ReturnType<typeof vi.fn>;
    fetchMock.mockResolvedValue(response(boardState({ eventCount: 2 })));
    fetchMock.mockResolvedValueOnce(response(boardState({ eventCount: 1 })));
    fetchMock.mockResolvedValueOnce(response(boardState({ output: 200, eventCount: 2 })));
    const capture: { current: PlanBoardStateResult | null } = { current: null };
    const rendered = await render(<Probe revision={7} capture={capture} />);
    await act(async () => flush());

    expect(fetchMock).toHaveBeenCalledTimes(1);
    for (const event of [
      "mem-activity",
      "graph-worker-spawned",
      "graph-worker-progress",
      "graph-worker-done",
      "graph-worker-reported",
      "cron-run-finished",
      "config-changed",
      "plan-changed",
    ]) {
      expect(listeners.get(event)?.size).toBe(1);
    }

    await act(async () => {
      emit("plan-changed", { plan_id: "pl_other" });
      await flush();
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      emit("mem-activity", { op: "write" });
      await flush();
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(capture.current?.state?.mission_control.event_log.recorded_events).toBe(2);

    await rendered.unmount();
    for (const set of listeners.values()) expect(set.size).toBe(0);

    const remounted = await render(<Probe revision={7} capture={capture} />);
    await act(async () => flush());
    for (const event of [
      "mem-activity",
      "graph-worker-spawned",
      "graph-worker-progress",
      "graph-worker-done",
      "graph-worker-reported",
      "cron-run-finished",
      "config-changed",
      "plan-changed",
    ]) {
      expect(listeners.get(event)?.size).toBe(1);
    }
    await remounted.unmount();
  });

  it("mission_control_keeps_last_snapshot_during_refresh", async () => {
    const fetchMock = globalThis.fetch as unknown as ReturnType<typeof vi.fn>;
    const nextRead = deferred<Response>();
    fetchMock.mockResolvedValueOnce(response(boardState({ output: 100, eventCount: 1 })));
    fetchMock.mockImplementationOnce(() => nextRead.promise);
    const capture: { current: PlanBoardStateResult | null } = { current: null };
    const rendered = await render(<Probe revision={7} capture={capture} />);
    await act(async () => flush());

    await act(async () => {
      emit("graph-worker-progress", {});
      await flush();
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(rendered.host.querySelector('[data-testid="mc-board-card-grid"]')).not.toBeNull();
    expect(rendered.host.querySelector('[data-testid="mc-board-pending"]')).toBeNull();
    expect(text(rendered.host, "mc-board-tokens-amount")).toBe("100");
    expect(capture.current?.loading).toBe(true);

    nextRead.resolve(response(boardState({ output: 200, eventCount: 2 })));
    await act(async () => flush());
    await act(async () => {
      advanceFrames(0);
      advanceFrames(360);
    });
    expect(text(rendered.host, "mc-board-tokens-amount")).toBe("200");
    expect(capture.current?.state?.mission_control.event_log.recorded_events).toBe(2);
    await rendered.unmount();
  });

  it("mission_control_animates_changed_figures", async () => {
    const rendered = await render(
        <MissionControlBoard plan={PLAN} state={boardState({ output: 100, turns: 10, runway: 20, tasks: 1 })} now={NOW} />,
    );
    expect(text(rendered.host, "mc-board-tasks-amount")).toBe("1");
    expect(text(rendered.host, "mc-board-tokens-amount")).toBe("100");
    expect(text(rendered.host, "mc-board-turns-amount")).toBe("10");
    expect(rendered.host.querySelector(".ct-usage-gauge .pct")?.textContent).toBe("20%");

    await act(async () => {
      rendered.root.render(
        <MissionControlBoard plan={PLAN} state={boardState({ output: 200, turns: 30, bounces: 2, runway: 60, tasks: 10 })} now={NOW} />,
      );
      await flush();
    });
    expect(text(rendered.host, "mc-board-tokens-amount")).toBe("100");
    expect(text(rendered.host, "mc-board-turns-amount")).toBe("10");
    expect(text(rendered.host, "mc-board-tasks-amount")).toBe("1");
    expect(rendered.host.querySelector(".ct-usage-gauge .pct")?.textContent).toBe("20%");

    await act(async () => advanceFrames(0));
    await act(async () => advanceFrames(180));
    expect(text(rendered.host, "mc-board-tokens-amount")).not.toBe("100");
    expect(text(rendered.host, "mc-board-tokens-amount")).not.toBe("200");
    expect(text(rendered.host, "mc-board-tasks-amount")).not.toBe("1");
    expect(text(rendered.host, "mc-board-tasks-amount")).not.toBe("10");
    expect(rendered.host.querySelector(".ct-usage-gauge .pct")?.textContent).not.toBe("20%");
    expect(rendered.host.querySelector(".ct-usage-gauge .pct")?.textContent).not.toBe("60%");

    await act(async () => advanceFrames(1000));
    expect(text(rendered.host, "mc-board-tokens-amount")).toBe("200");
    expect(text(rendered.host, "mc-board-turns-amount")).toBe("30");
    expect(text(rendered.host, "mc-board-tasks-amount")).toBe("10");
    expect(rendered.host.querySelector(".ct-usage-gauge .pct")?.textContent).toBe("60%");
    await rendered.unmount();
  });

  it("mission_control_reduced_motion_updates_immediately", async () => {
    const rendered = await render(
      <MissionControlBoard plan={PLAN} state={boardState({ output: 100, runway: 20, tasks: 1 })} now={NOW} />,
    );
    mediaMatches = true;
    await act(async () => {
      for (const listener of [...mediaListeners]) listener();
      await flush();
    });

    await act(async () => {
      rendered.root.render(
        <MissionControlBoard plan={PLAN} state={boardState({ output: 200, runway: 60, tasks: 3 })} now={NOW} />,
      );
      await flush();
    });
    expect(text(rendered.host, "mc-board-tokens-amount")).toBe("200");
    expect(text(rendered.host, "mc-board-tasks-amount")).toBe("3");
    expect(rendered.host.querySelector(".ct-usage-gauge .pct")?.textContent).toBe("60%");
    expect(frames.size).toBe(0);
    await rendered.unmount();
  });
});
