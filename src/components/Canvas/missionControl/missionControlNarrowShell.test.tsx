import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { useSidebarViewport } from "../../../hooks/useSidebarViewport";
import { FIXTURE_PLAN } from "../cards/fixtures.testdata";
import { MissionControlBoard } from "./MissionControlBoard";
import type { MissionControlState, PlanBoardState } from "./missionControlState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PHONE_WIDTH = 390;
const DESKTOP_WIDTH = 1280;
const H = 3_600_000;
const DAY = 1_700_000_000_000;
const NOW = DAY + 6 * H;

const observers: Array<(width: number) => void> = [];

class WidthObserver {
  constructor(private readonly cb: ResizeObserverCallback) {
    observers.push((width) => {
      this.cb(
        [{ contentRect: { width } } as unknown as ResizeObserverEntry],
        this as unknown as ResizeObserver,
      );
    });
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}

(globalThis as { ResizeObserver?: unknown }).ResizeObserver = WidthObserver;

type MediaListener = (event: MediaQueryListEvent) => void;

const originalMatchMedia = window.matchMedia;

/** A resizable viewport, so the shell is exercised through the media query it really subscribes to. */
function installViewport(initialWidth: number) {
  let width = initialWidth;
  const subscriptions: Array<{ query: string; listeners: Set<MediaListener> }> = [];

  const evaluate = (query: string): boolean => {
    const max = /\(max-width:\s*(\d+)px\)/.exec(query);
    if (max) return width <= Number(max[1]);
    const min = /\(min-width:\s*(\d+)px\)/.exec(query);
    if (min) return width >= Number(min[1]);
    return false;
  };

  window.matchMedia = ((query: string) => {
    const listeners = new Set<MediaListener>();
    subscriptions.push({ query, listeners });
    return {
      media: query,
      get matches() {
        return evaluate(query);
      },
      onchange: null,
      addEventListener: (_type: string, listener: MediaListener) => listeners.add(listener),
      removeEventListener: (_type: string, listener: MediaListener) => listeners.delete(listener),
      addListener: (listener: MediaListener) => listeners.add(listener),
      removeListener: (listener: MediaListener) => listeners.delete(listener),
      dispatchEvent: () => true,
    } as unknown as MediaQueryList;
  }) as typeof window.matchMedia;

  return async (next: number) => {
    width = next;
    await act(async () => {
      for (const subscription of subscriptions) {
        for (const listener of subscription.listeners) {
          listener({
            matches: evaluate(subscription.query),
            media: subscription.query,
          } as MediaQueryListEvent);
        }
      }
    });
  };
}

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");

/** The shipped stylesheets, so the board's narrow state is read back through the real cascade. */
function installStylesheets() {
  const style = document.createElement("style");
  style.textContent = [
    readFileSync(path.join(REPO_ROOT, "src/styles/globals.css"), "utf8"),
    readFileSync(
      path.join(REPO_ROOT, "src/components/Canvas/missionControl/missionControlBoard.css"),
      "utf8",
    ),
  ].join("\n");
  document.head.append(style);
}

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return {
    host,
    setBoardWidth: async (width: number) => {
      await act(async () => {
        for (const report of observers) report(width);
      });
    },
    unmount: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

/** The shell's sidebar contract, driven by the same hook App mounts. */
function ShellHarness() {
  const sidebar = useSidebarViewport();
  return (
    <div
      data-testid="app-shell-body"
      data-shell-viewport={sidebar.phone ? "phone" : "desktop"}
      data-sidebar-open={sidebar.open ? "true" : "false"}
    >
      {sidebar.mounted && (
        <div data-testid="sidebar-drawer" data-shown={sidebar.shown ? "true" : "false"} />
      )}
    </div>
  );
}

function attr(host: HTMLElement, testId: string, name: string): string | null {
  return host.querySelector(`[data-testid="${testId}"]`)?.getAttribute(name) ?? null;
}

function text(host: HTMLElement, testId: string): string {
  return host.querySelector(`[data-testid="${testId}"]`)?.textContent?.trim() ?? "";
}

function find(host: HTMLElement, selector: string): HTMLElement {
  const el = host.querySelector(selector);
  if (!el) throw new Error(`the board rendered no ${selector}`);
  return el as HTMLElement;
}

/** Read once per element: the cascade is resolved on first read and not re-resolved afterwards. */
function styleOf(host: HTMLElement, selector: string) {
  const computed = getComputedStyle(find(host, selector));
  return {
    display: computed.display,
    whiteSpace: computed.whiteSpace,
    overflow: computed.overflow,
    textOverflow: computed.textOverflow,
    flexWrap: computed.flexWrap,
    flex: computed.flex,
  };
}

function missionControlState(): MissionControlState {
  return {
    held: {
      held_reviews: [
        { task_id: "t1", name: "Land the wave", verdicts: 5, last_verdict_at: NOW - H, age_ms: H },
      ],
      open_gates: [],
      oldest_hold_age_ms: H,
      verdicts_on_held: 5,
    },
    bounces: { total_attempts: 12, tasks_on_second_attempt: 1, first_try_done: 10, labelled_bounced: 2 },
    event_log: {
      recorded_events: 3,
      truncated: false,
      entries: [
        {
          ts: DAY + 2 * H,
          last_ts: DAY + 2 * H,
          count: 1,
          kind: "worker_report",
          author: "responsive-shell-architect",
          author_tab_id: "tab-sol",
          task_id: "t1",
          task_name: "Readable at 390px",
          text: "In Review",
        },
      ],
    },
    areas: {
      state: "declared",
      facet_key: "area",
      facet_label: "Area",
      groups: [{ value: "design", label: "Design", task_count: 6, task_ids: ["t1"] }],
      untagged_task_count: 0,
      role_fallback: [],
    },
    kpi: {
      state: "recording",
      not_recording_reason: null,
      spend: {
        events: 4023,
        bytes: 65_000_000,
        turns: 4023,
        tokens: { input: 1_400_000_000, uncached_input: 1_500_000, output: 5_300_000, total: 1_405_300_000 },
      },
    },
    runway: { state: "not_reporting", providers: [] },
  };
}

function boardState(): PlanBoardState {
  return {
    plan_id: FIXTURE_PLAN.id,
    task_counts: {
      pending: 1,
      in_progress: 0,
      in_review: 2,
      blocked: 0,
      paused: 0,
      done: 10,
      cancelled: 1,
      total: 14,
    },
    mission_control: missionControlState(),
  };
}

function board() {
  return <MissionControlBoard plan={FIXTURE_PLAN} state={boardState()} now={NOW} />;
}

beforeAll(installStylesheets);

afterEach(() => {
  observers.length = 0;
  window.matchMedia = originalMatchMedia;
});

describe("mission control narrow shell", () => {
  it("mission_control_narrow_shell_gives_board_usable_width", async () => {
    const setViewport = installViewport(DESKTOP_WIDTH);
    const shell = await render(<ShellHarness />);

    expect(attr(shell.host, "app-shell-body", "data-shell-viewport")).toBe("desktop");
    expect(attr(shell.host, "app-shell-body", "data-sidebar-open")).toBe("true");
    expect(shell.host.querySelector('[data-testid="sidebar-drawer"]')).not.toBeNull();

    await setViewport(PHONE_WIDTH);

    expect(attr(shell.host, "app-shell-body", "data-shell-viewport")).toBe("phone");
    expect(attr(shell.host, "app-shell-body", "data-sidebar-open")).toBe("false");
    expect(attr(shell.host, "sidebar-drawer", "data-shown")).not.toBe("true");

    await setViewport(DESKTOP_WIDTH);

    expect(attr(shell.host, "app-shell-body", "data-shell-viewport")).toBe("desktop");
    expect(attr(shell.host, "app-shell-body", "data-sidebar-open")).toBe("true");
    await shell.unmount();

    await setViewport(PHONE_WIDTH);
    const phone = await render(<ShellHarness />);
    expect(attr(phone.host, "app-shell-body", "data-sidebar-open")).toBe("false");
    expect(phone.host.querySelector('[data-testid="sidebar-drawer"]')).toBeNull();
    await phone.unmount();

    const narrow = await render(board());
    await narrow.setBoardWidth(PHONE_WIDTH);

    expect(attr(narrow.host, "mission-control-board", "data-board-layout")).toBe("narrow");
    expect(text(narrow.host, "plan-board-plan-title")).toBe(FIXTURE_PLAN.title);
    expect(text(narrow.host, "mc-board-progress")).toContain("10 of 13 done");
    expect(text(narrow.host, "mc-board-tokens-amount")).toBe("5.3M");
    expect(text(narrow.host, "mc-board-turns-amount")).toBe("4,023");
    expect(narrow.host.querySelector('[data-testid="mission-control-board-actions"]')).not.toBeNull();

    const row = find(narrow.host, '[data-testid="mc-board-event-row"]');
    expect(row.querySelector(".mc-board-log-time")?.textContent).toMatch(/\d/);
    expect(row.textContent).toContain("Readable at 390px");
    expect(row.textContent).toContain("responsive-shell-architect");

    // Nothing the reader needs is truncated away at phone width.
    expect(styleOf(narrow.host, ".mc-board-log-message")).toMatchObject({
      whiteSpace: "normal",
      overflow: "visible",
      textOverflow: "clip",
    });
    expect(styleOf(narrow.host, ".mc-board-log-author").whiteSpace).toBe("normal");
    expect(styleOf(narrow.host, ".plan-board-done-count")).toMatchObject({
      whiteSpace: "normal",
      flex: "1 1 100%",
    });
    expect(styleOf(narrow.host, ".plan-board-status-topline").flexWrap).toBe("wrap");
    expect(styleOf(narrow.host, ".mc-board-log-row").display).toBe("grid");
    await narrow.unmount();

    // Desktop keeps the dense single-line log it was designed with.
    const wide = await render(board());
    await wide.setBoardWidth(DESKTOP_WIDTH);

    expect(attr(wide.host, "mission-control-board", "data-board-layout")).toBe("wide");
    expect(styleOf(wide.host, ".mc-board-log-message")).toMatchObject({
      whiteSpace: "nowrap",
      overflow: "hidden",
      textOverflow: "ellipsis",
    });
    expect(styleOf(wide.host, ".plan-board-done-count").whiteSpace).toBe("nowrap");
    expect(styleOf(wide.host, ".mc-board-log-row").display).toBe("contents");
    await wide.unmount();
  });
});
