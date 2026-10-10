import { strict as assert } from "node:assert";
import { test } from "node:test";
import { cronReportTitle, terminalOrigin } from "./teamRowLabels";

test("a live creator is named by its title, falling back to a neutral label", () => {
  assert.deepEqual(terminalOrigin("tab-a", "Hugo", true), { kind: "tab", id: "tab-a", label: "Hugo" });
  assert.deepEqual(terminalOrigin("tab-a", "  ", true), { kind: "tab", id: "tab-a", label: "Untitled tab" });
});

test("a closed or missing creator never surfaces its id as the label", () => {
  assert.deepEqual(terminalOrigin("tab-gone", null, false), { kind: "closed", id: "tab-gone" });
  assert.deepEqual(terminalOrigin(null, null, false), { kind: "unknown" });
});

test("a cron's report target is named by tab title, never by tab id", () => {
  assert.equal(cronReportTitle("tab-a", "Hugo"), "Reports to Hugo");
  assert.ok(!cronReportTitle("tab-gone", undefined).includes("tab-gone"));
  assert.equal(cronReportTitle(null, null), "Runs in a private cron worker");
});
