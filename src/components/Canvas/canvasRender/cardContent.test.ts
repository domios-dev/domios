// Run: npx tsx src/components/Canvas/canvasRender/cardContent.test.ts

import { FIXTURE_PLAN, FIXTURE_TAG_FACETS, FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { Plan } from "../types";
import { buildCardContent, MIN_NEAR_H, MIN_NEAR_W, pickLevel } from "./cardContent";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}
const hasSection = (c: { sections: { title: string }[] }, title: string) => c.sections.some((s) => s.title === title);

// Plan content: kicker + title + section titles + plan tag legend.
{
  const c = buildCardContent("plan", FIXTURE_PLAN)!;
  ok("plan kicker", c.kicker === "PLAN");
  ok("plan title", c.title === "Auth rollout");
  ok("plan lists flat section titles", hasSection(c, "Authentication") && hasSection(c, "OAuth flow"));
  ok("plan top tags from color-lane facet", c.tags.some((t) => t.text === "Navigation"));
}

// Spec content: section titles each get a "→ tasks" button, no fields.
{
  const specModel: Plan = { ...FIXTURE_PLAN, tasks: [] };
  const c = buildCardContent("spec", specModel)!;
  ok("spec kicker", c.kicker === "SPEC");
  ok("spec section has button", c.sections.length > 0 && c.sections.every((s) => s.button === "→ tasks"));
  ok("spec has no fields", c.fields.length === 0);
}

// Task content: title + tags (faceted label + topic) + status/priority/assignee fields.
{
  const c = buildCardContent("task", { task: FIXTURE_TASK, tagFacets: [...FIXTURE_TAG_FACETS] })!;
  ok("task kicker", c.kicker === "TASK");
  ok("task title is the name", c.title === "Dante");
  ok("task tag resolves facet label", c.tags.some((t) => t.text === "Navigation"));
  ok("task tag keeps topic", c.tags.some((t) => t.text === "pan-zoom"));
  const labels = c.fields.map((f) => f.label);
  ok("task fields status/priority/executor", labels.includes("status") && labels.includes("priority") && (labels.includes("executor") || labels.includes("assignee")));
  ok("task status field uses status_label", c.fields.find((f) => f.label === "status")?.value === "90% done");
}

// Level: FAR when small; NEAR only when the card fits ALL near content with no scroll.
{
  const c = buildCardContent("task", { task: FIXTURE_TASK, tagFacets: [...FIXTURE_TAG_FACETS] })!;
  ok("far when too narrow", pickLevel(c, MIN_NEAR_W - 1, 9999) === "far");
  ok("far when too short", pickLevel(c, 400, MIN_NEAR_H - 10) === "far");
  ok("near when big enough", pickLevel(c, 400, MIN_NEAR_H + 10) === "near");
}

// Deep band needs the readable body on the content model.
{
  const c = buildCardContent("task", { task: FIXTURE_TASK, tagFacets: [...FIXTURE_TAG_FACETS] })!;
  ok("task content carries the body for deep-read", c.body === FIXTURE_TASK.body.trim());
  const empty = buildCardContent("task", { task: { ...FIXTURE_TASK, body: "" }, tagFacets: [...FIXTURE_TAG_FACETS] })!;
  ok("task with no body → undefined (drawn as 'No description')", empty.body === undefined);
}

// Done task shows last executor metadata, not a raw pane id assignee field.
{
  const done = buildCardContent("task", {
    task: { ...FIXTURE_TASK, status: "done", assigned_pane_id: "pane-42", taken_by_role: "backend-dev", model: "gpt-5.5" },
    historicalExecutor: {
      label: "Puccini",
      role: "backend-dev",
      model: "gpt-5.5",
      branch: null,
      reportStatus: "done",
      outcome: "done",
      attemptCount: 1,
      attempts: [],
      summary: "Puccini · backend-dev · gpt-5.5 · done",
    },
  })!;
  ok("done task uses last executor field", done.fields.some((f) => f.label === "last executor"));
  ok("done task summary on card", done.fields.find((f) => f.label === "last executor")?.value.includes("Puccini"));
}

// A node with no sections/fields (e.g. unknown kind) stays far / null content.
{
  ok("unknown kind has no content", buildCardContent("note", { content: "x" }) === null);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
