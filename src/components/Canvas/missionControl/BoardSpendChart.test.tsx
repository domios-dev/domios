import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SpendChart } from "./BoardSpendChart";
import type { SpendChartData } from "./spendChart";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const format = (value: number) => String(value);
let root: Root | null = null;
let host: HTMLElement | null = null;

async function render(node: React.ReactElement): Promise<HTMLElement> {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(node));
  return host;
}

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

const boardData: SpendChartData = {
  days: [
    { start: 1, label: "Mon", today: false, total: 30, segments: [{ key: "a", label: "a", tone: 0, value: 30 }] },
    { start: 2, label: "Tue", today: true, total: 0, segments: [] },
  ],
  legend: [],
};

describe("spend chart", () => {
  it("keeps the Board's static reading when no coverage or selection is supplied", async () => {
    const view = await render(<SpendChart data={boardData} label="new input" format={format} />);
    const bars = view.querySelector(".mc-board-chart-bars");
    expect(bars?.getAttribute("role")).toBe("img");
    expect(bars?.getAttribute("aria-label")).toBe("new input by day: Mon 30, Tue 0");
    expect(view.querySelector("button")).toBeNull();
    expect(view.querySelector("[data-coverage]")).toBeNull();
    const stacks = Array.from(view.querySelectorAll<HTMLElement>(".mc-board-chart-stack"));
    expect(stacks.map((stack) => stack.style.height)).toEqual(["100%", "0%"]);
  });

  it("draws an unrecorded bucket without a figure and scales against recorded buckets only", async () => {
    const data: SpendChartData = {
      days: [
        { start: 1, label: "Sep 1", today: false, total: 0, segments: [], coverage: "not_recorded" },
        {
          start: 2,
          label: "Sep 8",
          today: false,
          total: 40,
          note: "from 17:50",
          coverage: "partial",
          segments: [
            { key: "p", label: "one", tone: 0, value: 30 },
            { key: "q", label: "rest", tone: "other", value: 10 },
          ],
        },
        { start: 3, label: "Sep 15", today: true, total: 20, segments: [{ key: "u", label: "unnamed", tone: "unnamed", value: 20 }] },
      ],
      legend: [{ key: "nr", label: "not recorded", tone: "not_recorded" }],
    };
    const view = await render(<SpendChart data={data} label="New input" unit="week" format={format} />);
    const label = view.querySelector(".mc-board-chart-bars")?.getAttribute("aria-label");
    expect(label).toBe("New input by week: Sep 1 not recorded, Sep 8 40, Sep 15 20");
    const [unrecorded, partial, today] = Array.from(view.querySelectorAll<HTMLElement>(".mc-board-chart-stack"));
    expect(unrecorded!.dataset.coverage).toBe("not_recorded");
    expect(unrecorded!.querySelectorAll("[data-testid='mc-board-spend-chart-segment']")).toHaveLength(0);
    expect(unrecorded!.title).toBe("Sep 1 not recorded");
    expect(partial!.style.height).toBe("100%");
    expect(today!.style.height).toBe("50%");
    expect(view.textContent).toContain("from 17:50");
    const tones = Array.from(view.querySelectorAll("[data-testid='mc-board-spend-chart-segment']")).map((s) =>
      s.getAttribute("data-tone"),
    );
    expect(tones).toEqual(["0", "other", "unnamed"]);
    expect(view.querySelector("[data-testid='mc-board-spend-legend-item'] .mc-board-swatch")?.getAttribute("data-tone")).toBe(
      "not_recorded",
    );
  });

  it("reports the chosen bar through buttons when the surface asks for selection", async () => {
    const onSelectDay = vi.fn();
    const view = await render(
      <SpendChart data={boardData} label="new input" format={format} selectedStart={2} onSelectDay={onSelectDay} />,
    );
    expect(view.querySelector(".mc-board-chart-bars")?.getAttribute("role")).toBe("group");
    const buttons = Array.from(view.querySelectorAll<HTMLButtonElement>("[data-testid='mc-board-spend-chart-select']"));
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual(["Mon 30", "Tue 0"]);
    expect(buttons.map((button) => button.getAttribute("aria-pressed"))).toEqual(["false", "true"]);
    await act(async () => buttons[0]!.click());
    expect(onSelectDay).toHaveBeenCalledWith(1);
  });
});
