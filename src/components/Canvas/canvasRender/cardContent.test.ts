// Run: npx tsx src/components/Canvas/canvasRender/cardContent.test.ts

import { FIXTURE_PLAN, FIXTURE_TAG_FACETS, FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { Plan, Task } from "../types";
import { buildCardContent, MIN_NEAR_H, MIN_NEAR_W, pickLevel, specCoverage } from "./cardContent";

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

{
  const c = buildCardContent("plan", { ...FIXTURE_PLAN, mode: "paused" })!;
  ok("parked plan kicker", c.kicker === "PARKED");
}

// when tasks exist; no fields; coverage chips in the tags row.
{
  const specModel: Plan = { ...FIXTURE_PLAN, tasks: [] };
  const c = buildCardContent("spec", specModel)!;
  ok("spec kicker", c.kicker === "SPEC");
  ok("spec has sections", c.sections.length > 0);
  ok("spec no-task sections have null button (no identical '→ tasks' spam)", c.sections.every((s) => s.button === null));
  ok("spec has no fields", c.fields.length === 0);
  ok("spec with no tasks: all sections uncovered", c.sections.every((s) => s.glyph === "uncovered"));
}

// Spec with tasks: task-count buttons only on covered sections, coverage chips in tags.
{
  const specWithTasks: Plan = { ...FIXTURE_PLAN }; // FIXTURE_PLAN has 1 task on sec-auth
  const c = buildCardContent("spec", specWithTasks)!;
  const secAuth = c.sections.find((s) => s.sectionId === "sec-auth");
  ok("spec covered section has task-count button", secAuth?.button?.includes("task") === true);
  ok("spec uncovered subsection has null button", c.sections.find((s) => s.sectionId === "sec-auth-oauth")?.button === null);
  ok("spec covered section glyph is progress (in_progress task)", secAuth?.glyph === "progress");
  ok("spec tags row has coverage chips", c.tags.length > 0);
}

// specCoverage: direct coverage computation.
{
  const noTaskSpec: Plan = { ...FIXTURE_PLAN, tasks: [] };
  const covEmpty = specCoverage(noTaskSpec);
  ok("specCoverage total = section count (flat)", covEmpty.total === 2); // sec-auth + sec-auth-oauth
  ok("specCoverage all unplanned when no tasks", covEmpty.unplanned === 2 && covEmpty.covered === 0 && covEmpty.partial === 0);

  const covPartial = specCoverage(FIXTURE_PLAN);
  ok("specCoverage partial when in_progress task", covPartial.partial === 1);
  ok("specCoverage unplanned subsection not covered by parent task", covPartial.unplanned >= 1);

  const doneTask: Task = { ...FIXTURE_TASK, status: "done" };
  const donePlan: Plan = { ...FIXTURE_PLAN, tasks: [doneTask] };
  const covDone = specCoverage(donePlan);
  ok("specCoverage covered when task is done", covDone.covered >= 1);
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

{
  const c = buildCardContent("task", { task: FIXTURE_TASK, tagFacets: [...FIXTURE_TAG_FACETS], childCount: 2, childDone: 1 })!;
  ok("parent card rolls up open subtasks", c.fields.find((f) => f.label === "status")?.value === "blocked on subtasks");
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

// Done task shows last executor metadata, not a raw tab id assignee field.
{
  const done = buildCardContent("task", {
    task: { ...FIXTURE_TASK, status: "done", assigned_tab_id: "tab-42", taken_by_role: "backend-dev", model: "gpt-5.5" },
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
