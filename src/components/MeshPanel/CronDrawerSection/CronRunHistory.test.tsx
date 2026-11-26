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
  const englishTime = new Intl.DateTimeFormat("en", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
  expect(row.querySelector("time")?.textContent).toBe(`Scheduled ${englishTime.format(scheduled)}`);
  expect([...row.querySelectorAll("time")].map((time) => time.dateTime)).toEqual([
    "2026-11-19T15:32:00.000Z", "2026-11-19T15:32:15.000Z", "2026-11-19T15:32:16.000Z",
  ]);
});

it("shows summaries for successful and failed runs and exposes the full truncated summary on hover", () => {
  const full = "Nothing appended. " + "Missing record. ".repeat(30) + "Last record missing.";
  const [failure, success] = render([
    { run_id: "failed", status: "Failed", summary: full, started_at: scheduled, finished_at: scheduled, tab_id: null, session_id: "retained" },
    { run_id: "success", status: "Success", summary: "Added two records", started_at: scheduled, finished_at: scheduled, tab_id: null },
  ]);
  expect(failure.querySelector("button")?.getAttribute("aria-label")).toContain("Failed");
  const preview = [...failure.querySelectorAll("[title]")].find(element => element.getAttribute("title") === full);
  expect(preview?.textContent).toMatch(/^Nothing appended\..*…$/);
  expect(preview?.textContent).not.toContain("Last record missing.");
  expect(failure.querySelector("button")?.getAttribute("aria-description")).toBe(full);
  expect(success.textContent).toContain("Success");
  expect(success.textContent).toContain("Added two records");
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
