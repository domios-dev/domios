import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AnalyticsGroupBy, AnalyticsQuery } from "@codeterm/plugin-sdk";
import type { QueryOutcome } from "../../src/plugin";
import { Dashboard } from "./Dashboard";
import { snapshotFor } from "./model/fixtures.testdata";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Answer = (query: AnalyticsQuery) => QueryOutcome;

let root: Root | null = null;
let host: HTMLElement | null = null;
const calls: AnalyticsQuery[] = [];
const close = vi.fn();

function installBridge(answer: Answer) {
  (window as unknown as { ct: unknown }).ct = {
    invoke: vi.fn(async (method: string, args: { query: AnalyticsQuery }) => {
      expect(method).toBe("query");
      calls.push(args.query);
      return answer(args.query);
    }),
    close,
  };
}

function setWidth(narrow: boolean) {
  window.matchMedia = ((query: string) =>
    ({ matches: narrow, media: query, addEventListener() {}, removeEventListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia;
}

const snapshots: Answer = (query) => ({ kind: "snapshot", snapshot: snapshotFor(query.group_by as AnalyticsGroupBy) });

async function settle() {
  for (let i = 0; i < 4; i += 1) await act(async () => {});
}

async function render(answer: Answer, narrow = false) {
  setWidth(narrow);
  installBridge(answer);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Dashboard />));
  await settle();
  return host;
}

function text(view: HTMLElement, testId: string): string | undefined {
  return view.querySelector(`[data-testid="${testId}"]`)?.textContent ?? undefined;
}

function rowLabels(view: HTMLElement): string[] {
  return Array.from(view.querySelectorAll('[data-testid="analytics-breakdown-row"] th > span:not([aria-hidden])')).map((cell) => cell.textContent ?? "");
}

beforeEach(() => {
  calls.length = 0;
  close.mockReset();
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("analytics dashboard", () => {
  it("reads measured totals, groups providers with their models and asks only the host capability", async () => {
    const view = await render(snapshots);
    expect(view.querySelector("main")?.dataset.view).toBe("data");
    expect(text(view, "analytics-total-new-input-value")).toBe("300");
    expect(text(view, "analytics-total-turns-value")).toBe("3");
    expect(text(view, "analytics-total-tabs-value")).toBe("4");
    expect(rowLabels(view)).toEqual(["Alpha", "a-large", "Beta", "Model not reported"]);
    expect(new Set(calls.map((query) => query.group_by))).toEqual(new Set(["provider_model", "workspace", "team", "plan"]));
    expect(calls.every((query) => query.scope?.kind === "machine")).toBe(true);
    expect(view.querySelector('[data-testid="analytics-chart"] [role="group"]')?.getAttribute("aria-label")).toContain("not recorded");
  });

  it("switches the breakdown to workspaces from two workspaces", async () => {
    const view = await render(snapshots);
    const workspaceTab = view.querySelector<HTMLButtonElement>('[data-testid="analytics-group-by"] [data-value="workspace"]');
    await act(async () => workspaceTab!.click());
    await settle();
    expect(rowLabels(view)).toEqual(["Workspace A", "Workspace B"]);
  });

  it("sends a chosen filter as its own query and keeps the unfiltered choices", async () => {
    const view = await render(snapshots);
    await act(async () => view.querySelector<HTMLButtonElement>('[data-testid="analytics-filter-workspaces"]')!.click());
    const options = Array.from(view.querySelectorAll<HTMLButtonElement>('[data-testid="analytics-option-workspaces"]'));
    expect(options.map((option) => option.textContent)).toEqual(["Workspace A", "Workspace B"]);
    calls.length = 0;
    await act(async () => options[1]!.click());
    await settle();
    expect(calls.some((query) => JSON.stringify(query.filters) === JSON.stringify({ workspaces: ["ws-b"] }))).toBe(true);
    expect(text(view, "analytics-scope")).toContain("1 filter applied");
  });

  it("hides figures instead of showing zeros when the host cannot answer", async () => {
    const view = await render(() => ({ kind: "unavailable" }));
    expect(view.querySelector('[data-testid="analytics-host-unavailable"]')).not.toBeNull();
    expect(view.querySelector('[data-testid^="analytics-total-"]')).toBeNull();
    expect(view.querySelector('[data-testid="analytics-breakdown"]')).toBeNull();
  });

  it("shows dashes, not zeros, while the ledger is unreadable and still shows allowance", async () => {
    const view = await render((query) => {
      const snapshot = snapshotFor(query.group_by as AnalyticsGroupBy);
      return { kind: "snapshot", snapshot: { ...snapshot, recording: { ...snapshot.recording, state: "unavailable", reason: "database is locked" } } };
    });
    expect(text(view, "analytics-ledger-unavailable")).toContain("database is locked");
    expect(text(view, "analytics-total-new-input-value")).toBe("—");
    expect(text(view, "analytics-total-tabs-value")).toBe("—");
    expect(view.querySelectorAll('[data-testid="analytics-allowance-window"] [role="meter"]')).toHaveLength(2);
  });

  it("returns to the workspace on Escape, closing an open filter first", async () => {
    const view = await render(snapshots);
    await act(async () => view.querySelector<HTMLButtonElement>('[data-testid="analytics-filter-providers"]')!.click());
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(view.querySelector('[role="dialog"]')).toBeNull();
    expect(close).not.toHaveBeenCalled();
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("uses a modal filter sheet and a three-column table at phone width", async () => {
    const view = await render(snapshots, true);
    expect(view.querySelector("main")?.dataset.layout).toBe("narrow");
    expect(view.querySelectorAll(".analytics-breakdown thead th")).toHaveLength(3);
    await act(async () => view.querySelector<HTMLButtonElement>('[data-testid="analytics-filters-open"]')!.click());
    const sheet = view.querySelector('[data-testid="analytics-filter-sheet"]');
    expect(sheet?.getAttribute("aria-modal")).toBe("true");
    expect(sheet?.querySelectorAll("section")).toHaveLength(5);
  });
});
