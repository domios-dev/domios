import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import type { CronRunInfo } from "../../../lib/types";
import { CronRunHistory } from "./CronRunHistory";

const scheduled = Date.parse("2026-11-19T15:32:00Z");

function render(runs: CronRunInfo[]) {
  const container = document.createElement("div");
  container.innerHTML = renderToStaticMarkup(<CronRunHistory runs={runs} allowed onOpen={vi.fn()} />);
  return [...container.querySelectorAll("li")];
}

it("shows schedule, fire and admission timestamps and marks a late run", () => {
  const [row] = render([{
    run_id: "late", status: "Success", tab_id: null, finished_at: null,
    scheduled_occurrence_at: scheduled, fired_at: scheduled + 15_000,
    started_at: scheduled + 16_000, late_by_ms: 16_000,
  }]);

  expect(row.textContent).toContain("Late by 16s");
  expect([...row.querySelectorAll("time")].map((time) => time.dateTime)).toEqual([
    "2026-11-19T15:32:00.000Z", "2026-11-19T15:32:15.000Z", "2026-11-19T15:32:16.000Z",
  ]);
});

it("shows the missed occurrence without a fire time and keeps manual and historical times readable", () => {
  const [missed, manual, prior] = render([
    { run_id: "missed", status: "Cancelled", scheduled_occurrence_at: scheduled, started_at: scheduled + 601_000, finished_at: scheduled + 601_000, tab_id: null },
    { run_id: "manual", status: "Success", fired_at: scheduled, started_at: scheduled, finished_at: null, tab_id: null },
    { run_id: "prior", status: "Success", started_at: scheduled, finished_at: null, tab_id: null },
  ]);

  expect(missed.textContent).toContain("Missed");
  expect(missed.textContent).toContain("Recorded");
  expect(missed.textContent).not.toContain("Fired");
  expect(manual.textContent).not.toContain("Scheduled");
  expect(manual.textContent).toContain("Fired and admitted");
  expect(prior.textContent).toContain("Started");
});
