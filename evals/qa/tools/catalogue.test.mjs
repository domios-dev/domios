import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import {
  listCases,
  renderPlan,
  validateCatalogue,
  VERDICTS,
  writeReleaseMatrix
} from "./catalogue.mjs";

const QA_ROOT = resolve("evals/qa");

function caseMarkdown(entry) {
  const list = values => values.length > 0 ? values.join(", ") : "none";
  const charter = entry.tier === "exploratory-charter";
  return [
    "# " + entry.id + " — " + entry.title,
    "",
    "## Purpose",
    "",
    charter ? "Mission: explore the fixture behavior." : "Prove the fixture behavior.",
    "",
    "## Covers",
    "",
    "Plan task keys: none recorded.",
    "",
    "Defects: none recorded.",
    "",
    "## Tier",
    "",
    entry.tier,
    "",
    "## Surfaces and platforms",
    "",
    "- **Surfaces:** " + list(entry.surfaces),
    "- **Platforms:** " + list(entry.platforms),
    "- **Providers:** " + list(entry.providers),
    "- **Grants:** " + list(entry.grants),
    "- **Automation:** " + entry.automation,
    "- **Status:** " + entry.status,
    "",
    "## Preconditions and grants",
    "",
    charter ? "Scope: the owned fixture." : "The fixture is owned.",
    "",
    "## Fixture",
    "",
    charter ? "Time box: 20 minutes. A disposable fixture." : "A disposable fixture.",
    "",
    "## Steps",
    "",
    charter ? "Heuristics: try normal and unusual inputs." : "1. Run the fixture.",
    "",
    "## Oracle",
    "",
    "PASS — the owned fixture passes.",
    "",
    "FAIL — the owned fixture fails.",
    "",
    "NOT RUN — the owned fixture was unavailable.",
    "",
    "## Evidence to record",
    "",
    "Record the exact result.",
    "",
    "## Cleanup",
    "",
    "Remove the owned fixture.",
    "",
    "## History of notable runs/defects",
    "",
    charter
      ? "No notable runs or defects recorded yet. Regression follow-up: turn every confirmed defect into an indexed regression case."
      : "No notable runs or defects recorded yet.",
    ""
  ].join("\n");
}

function defaultCase(overrides = {}) {
  return {
    id: "CUR-001",
    title: "Basic delivery",
    area: "messaging",
    tier: "release-core",
    surfaces: ["cli", "web"],
    platforms: ["windows"],
    providers: [],
    grants: [],
    automation: "manual",
    case: "cases/CUR-001-basic-delivery/case.md",
    status: "active",
    ...overrides
  };
}

async function createFixture(entries) {
  const root = await mkdtemp(join(tmpdir(), "qa-catalogue-"));
  await mkdir(join(root, "cases"), { recursive: true });
  await mkdir(join(root, "areas"), { recursive: true });
  for (const name of ["catalogue.schema.json", "results.schema.json"]) {
    await writeFile(join(root, name), await readFile(join(QA_ROOT, name), "utf8"));
  }
  await writeFile(
    join(root, "catalogue.json"),
    JSON.stringify({ schema_version: 1, cases: entries }, null, 2) + "\n"
  );
  for (const entry of entries) {
    const directory = join(root, entry.case.split("/").slice(0, 2).join("/"));
    await mkdir(directory, { recursive: true });
    await writeFile(join(directory, "case.md"), caseMarkdown(entry));
  }
  const areas = new Map();
  for (const entry of entries) {
    const values = areas.get(entry.area) || [];
    values.push(entry);
    areas.set(entry.area, values);
  }
  for (const [area, areaCases] of areas) {
    const lines = [
      "# " + area,
      "",
      "## Cases",
      ""
    ];
    for (const entry of areaCases) {
      lines.push("- [" + entry.id + "](../" + entry.case + ")");
    }
    await writeFile(join(root, "areas", area + ".md"), lines.join("\n") + "\n");
  }
  return root;
}

function resultRow(overrides = {}) {
  return {
    id: "CUR-001",
    platform: "windows",
    surface: "cli",
    build_sha: "a1b2c3d4",
    boot_id: "boot-123",
    verdict: "PASS",
    started_utc: "2026-10-19T10:00:00Z",
    finished_utc: "2026-10-19T10:00:02Z",
    evidence: ["target-check/evals/qa/run-01/CUR-001/cli.json"],
    notes: "",
    ...overrides
  };
}

test("validates the pilot catalogue and its case folders", async () => {
  await assert.doesNotReject(validateCatalogue(QA_ROOT));
});

test("area pages may show the indexed case title after its ID link", async t => {
  const root = await createFixture([defaultCase()]);
  t.after(() => rm(root, { recursive: true, force: true }));
  const areaPath = join(root, "areas", "messaging.md");
  await writeFile(
    areaPath,
    "# messaging\n\n- [CUR-001](../cases/CUR-001-basic-delivery/case.md) — Basic delivery\n"
  );
  const result = await validateCatalogue(root);
  assert.equal(result.total, 1);
});

test("lists cases by tier, surface, platform, and area", async t => {
  const entries = [
    defaultCase(),
    defaultCase({
      id: "CUR-002",
      title: "Responsive navigation",
      tier: "regression",
      surfaces: ["phone", "web"],
      platforms: ["macos"],
      case: "cases/CUR-002-responsive-navigation/case.md"
    }),
    defaultCase({
      id: "CUR-003",
      title: "Team charter",
      area: "coordination",
      tier: "exploratory-charter",
      surfaces: ["team"],
      platforms: ["linux"],
      case: "cases/CUR-003-team-charter/case.md"
    })
  ];
  const root = await createFixture(entries);
  t.after(() => rm(root, { recursive: true, force: true }));

  const result = await listCases(root, {
    id: "CUR-002",
    tier: "regression",
    surface: "phone",
    platform: "macos",
    area: "messaging"
  });
  assert.deepEqual(result.map(entry => entry.id), ["CUR-002"]);
});

test("rejects case-title drift and missing IDs", async t => {
  const root = await createFixture([
    defaultCase(),
    defaultCase({
      id: "CUR-003",
      title: "Later case",
      case: "cases/CUR-003-later-case/case.md"
    })
  ]);
  t.after(() => rm(root, { recursive: true, force: true }));
  const firstPath = join(root, "cases", "CUR-001-basic-delivery", "case.md");
  const contents = await readFile(firstPath, "utf8");
  await writeFile(firstPath, contents.replace("Basic delivery", "Changed title"));
  await assert.rejects(
    validateCatalogue(root),
    /heading title does not match catalogue title|case IDs must remain contiguous/
  );
});

test("rejects an area page that points to the wrong case folder", async t => {
  const root = await createFixture([defaultCase()]);
  t.after(() => rm(root, { recursive: true, force: true }));
  const areaPath = join(root, "areas", "messaging.md");
  const contents = await readFile(areaPath, "utf8");
  await writeFile(areaPath, contents.replace(
    "CUR-001-basic-delivery",
    "CUR-001-wrong-folder"
  ));
  await assert.rejects(validateCatalogue(root), /invalid ID link/);
});

test("release plan expands required rows and leaves them NOT RUN", async t => {
  const root = await createFixture([
    defaultCase(),
    defaultCase({
      id: "CUR-002",
      title: "Team charter",
      tier: "exploratory-charter",
      surfaces: ["team"],
      platforms: ["linux"],
      case: "cases/CUR-002-team-charter/case.md"
    })
  ]);
  t.after(() => rm(root, { recursive: true, force: true }));

  const plan = await renderPlan(root, "1.14.9");
  assert.equal((plan.match(/\| CUR-001 \|/g) || []).length, 2);
  assert.match(plan, /\| CUR-001 \| Basic delivery \| release-core \| windows \| cli \| NOT RUN \|/);
  assert.match(plan, /Optional exploratory charters/);
  assert.match(plan, /CUR-002 — Team charter/);
  assert.doesNotMatch(plan, /\| PASS \|/);
  await assert.rejects(renderPlan(root, "..\\outside"), /version must contain/);
});

test("merges results without converting missing targets to PASS", async t => {
  const entry = defaultCase({ surfaces: ["cli", "phone", "web"] });
  const root = await createFixture([entry]);
  t.after(() => rm(root, { recursive: true, force: true }));
  const resultPath = join(root, "results.json");
  await writeFile(resultPath, JSON.stringify({
    schema_version: 1,
    run_stamp: "release-1",
    results: [
      resultRow(),
      resultRow({
        surface: "phone",
        verdict: "OWNER-SKIPPED",
        evidence: [],
        notes: "Owner decision: no physical device was assigned for this release."
      })
    ]
  }, null, 2));

  const outputPath = await writeReleaseMatrix(root, "1.14.9", [resultPath]);
  const matrix = await readFile(outputPath, "utf8");
  assert.match(matrix, /\| CUR-001 \| \[Basic delivery\].*\| cli \| PASS \|/);
  assert.match(matrix, /\| CUR-001 \| \[Basic delivery\].*\| phone \| OWNER-SKIPPED \|/);
  assert.match(matrix, /\| CUR-001 \| \[Basic delivery\].*\| web \| NOT RUN \|/);
  assert.match(matrix, /Owner decision: no physical device was assigned for this release\./);
  assert.match(matrix, /## Owner scope-outs/);
});

test("uses only the five verdicts and rejects aliases", async t => {
  assert.deepEqual([...VERDICTS], ["PASS", "FAIL", "NOT RUN", "BLOCKED", "OWNER-SKIPPED"]);
  const root = await createFixture([defaultCase()]);
  t.after(() => rm(root, { recursive: true, force: true }));
  const resultPath = join(root, "invalid-results.json");
  await writeFile(resultPath, JSON.stringify({
    schema_version: 1,
    run_stamp: "bad-verdict",
    results: [resultRow({ verdict: "SKIP" })]
  }, null, 2));
  await assert.rejects(
    writeReleaseMatrix(root, "1.14.9", [resultPath]),
    /verdict must be exactly PASS, FAIL, NOT RUN, BLOCKED, OWNER-SKIPPED/
  );
});

test("rejects overlapping result files for the same matrix target", async t => {
  const root = await createFixture([defaultCase()]);
  t.after(() => rm(root, { recursive: true, force: true }));
  const firstPath = join(root, "first.json");
  const secondPath = join(root, "second.json");
  const envelope = { schema_version: 1, run_stamp: "run-a", results: [resultRow()] };
  await writeFile(firstPath, JSON.stringify(envelope));
  await writeFile(secondPath, JSON.stringify({ ...envelope, run_stamp: "run-b" }));
  await assert.rejects(
    writeReleaseMatrix(root, "1.14.9", [firstPath, secondPath]),
    /results files overlap target CUR-001\|windows\|cli/
  );
});
