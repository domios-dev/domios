import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const VERDICTS = Object.freeze([
  "PASS",
  "FAIL",
  "NOT RUN",
  "BLOCKED",
  "OWNER-SKIPPED"
]);

export const SURFACES = Object.freeze([
  "cli",
  "web",
  "native",
  "phone",
  "mesh",
  "team"
]);

export const TIERS = Object.freeze([
  "release-core",
  "regression",
  "exploratory-charter"
]);

const CASE_SECTIONS = Object.freeze([
  "Purpose",
  "Covers",
  "Tier",
  "Surfaces and platforms",
  "Preconditions and grants",
  "Fixture",
  "Steps",
  "Oracle",
  "Evidence to record",
  "Cleanup",
  "History of notable runs/defects"
]);

const REQUIRED_TIERS = new Set(["release-core", "regression"]);
const DEFAULT_QA_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const USAGE = [
  "Usage:",
  "  node evals/qa/tools/catalogue.mjs validate",
  "  node evals/qa/tools/catalogue.mjs list [--id ID] [--tier TIER] [--surface SURFACE] [--platform PLATFORM] [--area AREA]",
  "  node evals/qa/tools/catalogue.mjs plan VERSION",
  "  node evals/qa/tools/catalogue.mjs matrix VERSION RESULTS.json [RESULTS.json ...]"
].join("\n");

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function sameValues(left, right) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function expectKeys(record, expected, label, errors) {
  if (!isRecord(record)) {
    errors.push(label + " must be an object");
    return false;
  }
  const actual = Object.keys(record).sort();
  const wanted = expected.slice().sort();
  if (!sameValues(actual, wanted)) {
    errors.push(label + " keys must be exactly: " + wanted.join(", "));
  }
  return true;
}

function isUniqueStringList(value, label, errors, allowEmpty) {
  if (!Array.isArray(value) || (!allowEmpty && value.length === 0)) {
    errors.push(label + " must be " + (allowEmpty ? "an array" : "a non-empty array"));
    return false;
  }
  const seen = new Set();
  for (const item of value) {
    if (typeof item !== "string" || item.trim() !== item || item.length === 0) {
      errors.push(label + " values must be non-empty trimmed strings");
      return false;
    }
    if (seen.has(item)) {
      errors.push(label + " contains duplicate value " + item);
      return false;
    }
    seen.add(item);
  }
  return true;
}

function validateCatalogueShape(catalogue) {
  const errors = [];
  if (!expectKeys(catalogue, ["schema_version", "cases"], "catalogue", errors)) {
    return errors;
  }
  if (catalogue.schema_version !== 1) {
    errors.push("catalogue.schema_version must be 1");
  }
  if (!Array.isArray(catalogue.cases) || catalogue.cases.length === 0) {
    errors.push("catalogue.cases must be a non-empty array");
    return errors;
  }

  const seenIds = new Set();
  const idNumbers = [];
  const caseKeys = [
    "id",
    "title",
    "area",
    "tier",
    "surfaces",
    "platforms",
    "providers",
    "grants",
    "automation",
    "case",
    "status"
  ];
  for (const [index, entry] of catalogue.cases.entries()) {
    const label = "catalogue.cases[" + index + "]";
    if (!expectKeys(entry, caseKeys, label, errors)) {
      continue;
    }
    if (typeof entry.id !== "string" || !/^CUR-[0-9]{3}$/.test(entry.id)) {
      errors.push(label + ".id must match CUR-NNN");
    } else {
      if (seenIds.has(entry.id)) {
        errors.push("duplicate case id " + entry.id);
      }
      seenIds.add(entry.id);
      idNumbers.push(Number(entry.id.slice(4)));
    }
    if (typeof entry.title !== "string" || entry.title.trim() !== entry.title || entry.title.length < 3) {
      errors.push(label + ".title must be a non-empty title");
    }
    if (typeof entry.area !== "string" || !/^[a-z][a-z0-9-]*$/.test(entry.area)) {
      errors.push(label + ".area must be a lower-case slug");
    }
    if (!TIERS.includes(entry.tier)) {
      errors.push(label + ".tier must be one of " + TIERS.join(", "));
    }
    if (isUniqueStringList(entry.surfaces, label + ".surfaces", errors, false)) {
      for (const surface of entry.surfaces) {
        if (!SURFACES.includes(surface)) {
          errors.push(label + ".surfaces has unsupported value " + surface);
        }
      }
    }
    isUniqueStringList(entry.platforms, label + ".platforms", errors, false);
    isUniqueStringList(entry.providers, label + ".providers", errors, true);
    isUniqueStringList(entry.grants, label + ".grants", errors, true);
    if (
      typeof entry.automation !== "string" ||
      !["manual", "scripted"].includes(entry.automation) &&
        (
          !/^bench:evals\/[a-zA-Z0-9._/-]+$/.test(entry.automation) ||
          entry.automation.split("/").includes("..")
        )
    ) {
      errors.push(label + ".automation must be manual, scripted, or bench:evals/<path>");
    }
    if (
      typeof entry.case !== "string" ||
      !/^cases\/CUR-[0-9]{3}-[a-z0-9]+(?:-[a-z0-9]+)*\/case\.md$/.test(entry.case) ||
      !entry.case.startsWith("cases/" + entry.id + "-")
    ) {
      errors.push(label + ".case must be the matching CUR case folder path");
    }
    if (!["active", "retired"].includes(entry.status)) {
      errors.push(label + ".status must be active or retired");
    }
  }

  const sortedNumbers = idNumbers.slice().sort((left, right) => left - right);
  for (let index = 0; index < sortedNumbers.length; index += 1) {
    if (sortedNumbers[index] !== index + 1) {
      errors.push("case IDs must remain contiguous from CUR-001; missing CUR-" + String(index + 1).padStart(3, "0"));
      break;
    }
  }
  if (idNumbers.some((value, index) => value !== sortedNumbers[index])) {
    errors.push("catalogue cases must be ordered by stable ID");
  }
  return errors;
}

async function readJson(path, label) {
  let contents;
  try {
    contents = await readFile(path, "utf8");
  } catch (error) {
    throw new Error(label + " could not be read: " + error.message);
  }
  try {
    return JSON.parse(contents);
  } catch (error) {
    throw new Error(label + " is not valid JSON: " + error.message);
  }
}

function validateSchemaFiles(catalogueSchema, resultsSchema) {
  const errors = [];
  const draft = "https://json-schema.org/draft/2020-12/schema";
  if (!isRecord(catalogueSchema) || catalogueSchema.$schema !== draft) {
    errors.push("catalogue.schema.json must use JSON Schema 2020-12");
  }
  if (!isRecord(resultsSchema) || resultsSchema.$schema !== draft) {
    errors.push("results.schema.json must use JSON Schema 2020-12");
  }
  const schemaVerdicts = resultsSchema?.$defs?.verdict?.enum;
  if (!Array.isArray(schemaVerdicts) || !sameValues(schemaVerdicts, VERDICTS)) {
    errors.push("results.schema.json verdict enum must match the strict catalogue verdict vocabulary");
  }
  const caseSchema = catalogueSchema?.$defs?.case?.properties;
  if (!Array.isArray(caseSchema?.tier?.enum) || !sameValues(caseSchema.tier.enum, TIERS)) {
    errors.push("catalogue.schema.json tier enum must match the catalogue tool");
  }
  if (
    !Array.isArray(caseSchema?.surfaces?.items?.enum) ||
    !sameValues(caseSchema.surfaces.items.enum, SURFACES) ||
    !sameValues(resultsSchema?.$defs?.result?.properties?.surface?.enum || [], SURFACES)
  ) {
    errors.push("catalogue and results schemas must match the supported surfaces");
  }
  return errors;
}

function parseSectionBlocks(contents, label, errors) {
  const lines = contents.split(/\r?\n/);
  const firstContentLine = lines.find(line => line.trim().length > 0);
  const headingMatch = firstContentLine && /^# (CUR-[0-9]{3}) — (.+)$/.exec(firstContentLine);
  if (!headingMatch) {
    errors.push(label + " must start with '# CUR-NNN — title'");
  }

  const headings = [];
  for (let index = 0; index < lines.length; index += 1) {
    const match = /^## (.+?)\s*$/.exec(lines[index]);
    if (match) {
      headings.push({ name: match[1], index });
    }
  }
  if (!sameValues(headings.map(item => item.name), CASE_SECTIONS)) {
    errors.push(label + " must have the fixed sections in this order: " + CASE_SECTIONS.join("; "));
  }
  const sections = new Map();
  for (let index = 0; index < headings.length; index += 1) {
    const start = headings[index].index + 1;
    const end = index + 1 < headings.length ? headings[index + 1].index : lines.length;
    sections.set(headings[index].name, lines.slice(start, end).join("\n").trim());
  }
  return { headingMatch, sections };
}

function parseMetadataLine(section, key) {
  const matcher = new RegExp("^- \\*\\*" + key + ":\\*\\* (.+)$", "m");
  const match = matcher.exec(section);
  if (!match) {
    return null;
  }
  if (match[1] === "none") {
    return [];
  }
  return match[1].split(", ").map(value => value.trim());
}

function validateCaseDocument(entry, contents, errors) {
  const label = entry.case;
  const parsed = parseSectionBlocks(contents, label, errors);
  if (parsed.headingMatch) {
    if (parsed.headingMatch[1] !== entry.id) {
      errors.push(label + " heading ID does not match " + entry.id);
    }
    if (parsed.headingMatch[2] !== entry.title) {
      errors.push(label + " heading title does not match catalogue title");
    }
  }
  for (const section of CASE_SECTIONS) {
    const body = parsed.sections.get(section);
    if (!body) {
      errors.push(label + " has an empty or missing " + section + " section");
    }
  }
  const covers = parsed.sections.get("Covers") || "";
  if (!covers.includes("Plan task keys:") || !covers.includes("Defects:")) {
    errors.push(label + " Covers must state Plan task keys and Defects");
  }
  if (parsed.sections.get("Tier") !== entry.tier) {
    errors.push(label + " Tier section does not match catalogue tier");
  }
  const metadata = parsed.sections.get("Surfaces and platforms") || "";
  const expected = {
    Surfaces: entry.surfaces,
    Platforms: entry.platforms,
    Providers: entry.providers,
    Grants: entry.grants,
    Automation: [entry.automation],
    Status: [entry.status]
  };
  for (const [key, expectedValues] of Object.entries(expected)) {
    const actualValues = parseMetadataLine(metadata, key);
    if (!actualValues || !sameValues(actualValues, expectedValues)) {
      errors.push(label + " " + key + " metadata does not match catalogue.json");
    }
  }
  const oracle = parsed.sections.get("Oracle") || "";
  const oracleLabels = [...oracle.matchAll(/^(PASS|FAIL|NOT RUN) — .+$/gm)].map(match => match[1]);
  if (!sameValues([...new Set(oracleLabels)].sort(), ["FAIL", "NOT RUN", "PASS"])) {
    errors.push(label + " Oracle must state explicit PASS, FAIL, and NOT RUN conditions");
  }
  if (entry.tier === "exploratory-charter") {
    const charterFields = [
      "Mission:",
      "Scope:",
      "Time box:",
      "Heuristics:",
      "Regression follow-up:"
    ];
    for (const field of charterFields) {
      if (!contents.includes(field)) {
        errors.push(label + " exploratory charter must state " + field);
      }
    }
  }
}

async function validateAreaPages(root, cases, errors) {
  const directory = resolve(root, "areas");
  let files;
  try {
    files = (await readdir(directory, { withFileTypes: true }))
      .filter(entry => entry.isFile() && entry.name.endsWith(".md"))
      .map(entry => entry.name)
      .sort();
  } catch (error) {
    errors.push("areas directory could not be read: " + error.message);
    return;
  }
  const byArea = new Map();
  const entriesById = new Map(cases.map(entry => [entry.id, entry]));
  for (const entry of cases) {
    const values = byArea.get(entry.area) || [];
    values.push(entry);
    byArea.set(entry.area, values);
  }
  const pageAreas = new Set(files.map(file => basename(file, ".md")));
  for (const area of byArea.keys()) {
    if (!pageAreas.has(area)) {
      errors.push("missing area page areas/" + area + ".md");
    }
  }
  for (const area of pageAreas) {
    if (!byArea.has(area)) {
      errors.push("area page areas/" + area + ".md has no indexed cases");
    }
  }
  for (const file of files) {
    const area = basename(file, ".md");
    const contents = await readFile(resolve(directory, file), "utf8");
    const linked = [];
    for (const line of contents.split(/\r?\n/).filter(item => item.trim().startsWith("- "))) {
      const match = /^\s*-\s+\[(CUR-[0-9]{3})\]\(\.\.\/cases\/(CUR-[0-9]{3}-[a-z0-9-]+)\/case\.md\)\s*$/.exec(line);
      const expectedCase = match ? entriesById.get(match[1])?.case : undefined;
      if (
        !match ||
        match[1] !== match[2].slice(0, 7) ||
        expectedCase !== "cases/" + match[2] + "/case.md"
      ) {
        errors.push("areas/" + file + " has a case row with an invalid ID link: " + line.trim());
        continue;
      }
      linked.push(match[1]);
    }
    const expectedIds = (byArea.get(area) || []).map(entry => entry.id);
    if (!sameValues(linked, expectedIds)) {
      errors.push("areas/" + file + " case IDs do not match catalogue.json");
    }
  }
}

async function validateCaseFolders(root, cases, errors) {
  const directory = resolve(root, "cases");
  let folders;
  try {
    folders = await readdir(directory, { withFileTypes: true });
  } catch (error) {
    errors.push("cases directory could not be read: " + error.message);
    return;
  }
  const indexedPaths = new Set(cases.map(entry => entry.case.replaceAll("/", "\\").toLowerCase()));
  for (const folder of folders) {
    if (!folder.isDirectory()) {
      continue;
    }
    if (!/^CUR-[0-9]{3}-[a-z0-9]+(?:-[a-z0-9]+)*$/.test(folder.name)) {
      errors.push("cases contains an invalid case folder " + folder.name);
      continue;
    }
    const relativeCasePath = ("cases/" + folder.name + "/case.md").replaceAll("/", "\\").toLowerCase();
    if (!indexedPaths.has(relativeCasePath)) {
      errors.push("case folder is not indexed: " + folder.name);
    }
    try {
      await readFile(resolve(directory, folder.name, "case.md"), "utf8");
    } catch {
      errors.push("case folder is missing case.md: " + folder.name);
    }
  }
}

async function loadCatalogue(root = DEFAULT_QA_ROOT) {
  const catalogue = await readJson(resolve(root, "catalogue.json"), "catalogue.json");
  const catalogueSchema = await readJson(resolve(root, "catalogue.schema.json"), "catalogue.schema.json");
  const resultsSchema = await readJson(resolve(root, "results.schema.json"), "results.schema.json");
  return { catalogue, catalogueSchema, resultsSchema };
}

function throwIfErrors(errors, label = "Validation failed") {
  if (errors.length > 0) {
    throw new Error(label + ":\n- " + errors.join("\n- "));
  }
}

export async function validateCatalogue(root = DEFAULT_QA_ROOT) {
  const { catalogue, catalogueSchema, resultsSchema } = await loadCatalogue(root);
  const errors = [
    ...validateSchemaFiles(catalogueSchema, resultsSchema),
    ...validateCatalogueShape(catalogue)
  ];
  if (Array.isArray(catalogue?.cases)) {
    for (const entry of catalogue.cases) {
      if (
        typeof entry?.case !== "string" ||
        !/^cases\/CUR-[0-9]{3}-[a-z0-9]+(?:-[a-z0-9]+)*\/case\.md$/.test(entry.case) ||
        !entry.case.startsWith("cases/" + entry.id + "-")
      ) {
        continue;
      }
      try {
        const contents = await readFile(resolve(root, entry.case), "utf8");
        validateCaseDocument(entry, contents, errors);
      } catch (error) {
        errors.push(entry.case + " could not be read: " + error.message);
      }
    }
    await validateCaseFolders(root, catalogue.cases, errors);
    await validateAreaPages(root, catalogue.cases, errors);
  }
  throwIfErrors(errors, "Catalogue validation failed");
  const active = catalogue.cases.filter(entry => entry.status === "active").length;
  return {
    total: catalogue.cases.length,
    active,
    retired: catalogue.cases.length - active,
    areas: new Set(catalogue.cases.map(entry => entry.area)).size
  };
}

async function loadValidatedCatalogue(root) {
  await validateCatalogue(root);
  const { catalogue } = await loadCatalogue(root);
  return catalogue;
}

export async function listCases(root = DEFAULT_QA_ROOT, filters = {}) {
  const catalogue = await loadValidatedCatalogue(root);
  const allowedFilters = ["id", "tier", "surface", "platform", "area"];
  for (const key of Object.keys(filters)) {
    if (!allowedFilters.includes(key)) {
      throw new Error("unsupported list filter " + key);
    }
  }
  return catalogue.cases.filter(entry =>
    (!filters.id || entry.id === filters.id) &&
    (!filters.tier || entry.tier === filters.tier) &&
    (!filters.surface || entry.surfaces.includes(filters.surface)) &&
    (!filters.platform || entry.platforms.includes(filters.platform)) &&
    (!filters.area || entry.area === filters.area)
  );
}

export function renderList(cases) {
  const rows = ["ID\tTIER\tAREA\tSURFACES\tPLATFORMS\tSTATUS\tTITLE"];
  for (const entry of cases) {
    rows.push([
      entry.id,
      entry.tier,
      entry.area,
      entry.surfaces.join(","),
      entry.platforms.join(","),
      entry.status,
      entry.title
    ].join("\t"));
  }
  return rows.join("\n");
}

function validateVersion(version) {
  if (
    typeof version !== "string" ||
    !/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(version) ||
    version.includes("..")
  ) {
    throw new Error("version must contain only letters, numbers, dots, underscores, and hyphens");
  }
  return version;
}

export async function renderPlan(root = DEFAULT_QA_ROOT, version) {
  const safeVersion = validateVersion(version);
  const catalogue = await loadValidatedCatalogue(root);
  const rows = [
    "# Curated QA release plan: " + safeVersion,
    "",
    "Every planned row starts as NOT RUN. Record a result only after its oracle is proven.",
    "",
    "## Release checklist",
    "",
    "| ID | Case | Tier | Platform | Surface | Verdict |",
    "| --- | --- | --- | --- | --- | --- |"
  ];
  const charters = [];
  for (const entry of catalogue.cases.filter(item => item.status === "active")) {
    if (!REQUIRED_TIERS.has(entry.tier)) {
      charters.push(entry);
      continue;
    }
    for (const platform of entry.platforms) {
      for (const surface of entry.surfaces) {
        rows.push(
          "| " + entry.id + " | " + entry.title + " | " + entry.tier + " | " +
          platform + " | " + surface + " | NOT RUN |"
        );
      }
    }
  }
  rows.push("", "## Optional exploratory charters", "");
  if (charters.length === 0) {
    rows.push("None.");
  } else {
    for (const entry of charters) {
      rows.push("- [ ] " + entry.id + " — " + entry.title);
    }
  }
  return rows.join("\n") + "\n";
}

function validUtc(value) {
  return typeof value === "string" &&
    /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z$/.test(value) &&
    Number.isFinite(Date.parse(value));
}

function validateEvidence(evidence, label, errors) {
  if (!Array.isArray(evidence)) {
    errors.push(label + ".evidence must be an array");
    return;
  }
  for (const path of evidence) {
    if (
      typeof path !== "string" ||
      !path.startsWith("target-check/evals/qa/") ||
      path.includes("\\") ||
      path.split("/").includes("..")
    ) {
      errors.push(label + ".evidence entries must be relative target-check/evals/qa paths");
      return;
    }
  }
}

function validateResultEnvelope(envelope, catalogue, label) {
  const errors = [];
  if (!expectKeys(envelope, ["schema_version", "run_stamp", "results"], label, errors)) {
    return errors;
  }
  if (envelope.schema_version !== 1) {
    errors.push(label + ".schema_version must be 1");
  }
  if (
    typeof envelope.run_stamp !== "string" ||
    !/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(envelope.run_stamp)
  ) {
    errors.push(label + ".run_stamp must be a safe non-empty stamp");
  }
  if (!Array.isArray(envelope.results) || envelope.results.length === 0) {
    errors.push(label + ".results must be a non-empty array");
    return errors;
  }
  const caseById = new Map(catalogue.cases.map(entry => [entry.id, entry]));
  const seenRows = new Set();
  const resultKeys = [
    "id",
    "platform",
    "surface",
    "build_sha",
    "boot_id",
    "verdict",
    "started_utc",
    "finished_utc",
    "evidence",
    "notes"
  ];
  for (const [index, row] of envelope.results.entries()) {
    const rowLabel = label + ".results[" + index + "]";
    if (!expectKeys(row, resultKeys, rowLabel, errors)) {
      continue;
    }
    const entry = caseById.get(row.id);
    if (!entry || entry.status !== "active") {
      errors.push(rowLabel + ".id must name an active catalogue case");
    } else {
      if (!entry.platforms.includes(row.platform)) {
        errors.push(rowLabel + ".platform is not listed for " + row.id);
      }
      if (!entry.surfaces.includes(row.surface)) {
        errors.push(rowLabel + ".surface is not listed for " + row.id);
      }
    }
    const key = [row.id, row.platform, row.surface].join("|");
    if (seenRows.has(key)) {
      errors.push(label + " contains duplicate result target " + key);
    }
    seenRows.add(key);
    if (!VERDICTS.includes(row.verdict)) {
      errors.push(rowLabel + ".verdict must be exactly " + VERDICTS.join(", "));
    }
    if (typeof row.build_sha !== "string" || row.build_sha.trim().length === 0) {
      errors.push(rowLabel + ".build_sha must be recorded");
    }
    if (typeof row.boot_id !== "string" || row.boot_id.trim().length === 0) {
      errors.push(rowLabel + ".boot_id must be recorded");
    }
    if (!validUtc(row.started_utc) || !validUtc(row.finished_utc)) {
      errors.push(rowLabel + " timestamps must be UTC ISO-8601 strings ending in Z");
    } else if (Date.parse(row.finished_utc) < Date.parse(row.started_utc)) {
      errors.push(rowLabel + ".finished_utc must not precede started_utc");
    }
    validateEvidence(row.evidence, rowLabel, errors);
    if (typeof row.notes !== "string") {
      errors.push(rowLabel + ".notes must be a string");
    }
    if (
      ["PASS", "FAIL"].includes(row.verdict) &&
      Array.isArray(row.evidence) &&
      row.evidence.length === 0
    ) {
      errors.push(rowLabel + " " + row.verdict + " requires evidence");
    }
    if (
      ["NOT RUN", "BLOCKED"].includes(row.verdict) &&
      typeof row.notes === "string" &&
      row.notes.trim().length === 0
    ) {
      errors.push(rowLabel + " " + row.verdict + " requires a reason in notes");
    }
    if (
      row.verdict === "OWNER-SKIPPED" &&
      typeof row.notes === "string" &&
      !row.notes.startsWith("Owner decision: ")
    ) {
      errors.push(rowLabel + " OWNER-SKIPPED notes must start with 'Owner decision: '");
    }
  }
  return errors;
}

function escapeCell(value) {
  return String(value ?? "—")
    .replaceAll("|", "\\|")
    .replaceAll("\r\n", "<br>")
    .replaceAll("\n", "<br>");
}

function renderMatrix(version, catalogue, results) {
  const resultByTarget = new Map();
  for (const item of results) {
    for (const row of item.envelope.results) {
      resultByTarget.set(
        [row.id, row.platform, row.surface].join("|"),
        { ...row, run_stamp: item.envelope.run_stamp }
      );
    }
  }
  const rows = [
    "# Curated QA release matrix: " + version,
    "",
    "Generated from catalogue.json and the supplied results files. Missing targets remain NOT RUN.",
    "",
    "## Results",
    "",
    "| ID | Case | Tier | Platform | Surface | Verdict | Build / boot | Run stamp | Evidence | Notes |",
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
  ];
  const scopeOuts = [];
  const activeCases = catalogue.cases.filter(entry => entry.status === "active");
  for (const entry of activeCases) {
    for (const platform of entry.platforms) {
      for (const surface of entry.surfaces) {
        const key = [entry.id, platform, surface].join("|");
        const result = resultByTarget.get(key);
        const verdict = result ? result.verdict : "NOT RUN";
        const buildBoot = result ? result.build_sha + " / " + result.boot_id : "—";
        const runStamp = result ? result.run_stamp : "—";
        const evidence = result && result.evidence.length > 0 ? result.evidence.join("<br>") : "—";
        const notes = result ? result.notes : "No result supplied.";
        rows.push(
          "| " + entry.id +
          " | [" + escapeCell(entry.title) + "](../" + entry.case + ")" +
          " | " + entry.tier +
          " | " + platform +
          " | " + surface +
          " | " + verdict +
          " | " + escapeCell(buildBoot) +
          " | " + escapeCell(runStamp) +
          " | " + escapeCell(evidence) +
          " | " + escapeCell(notes) + " |"
        );
        if (verdict === "OWNER-SKIPPED") {
          scopeOuts.push(
            "| " + entry.id + " | " + platform + " / " + surface + " | " +
            escapeCell(notes) + " |"
          );
        }
      }
    }
  }
  rows.push(
    "",
    "## Owner scope-outs",
    "",
    "| ID | Target | Owner decision |",
    "| --- | --- | --- |"
  );
  rows.push(...(scopeOuts.length > 0 ? scopeOuts : ["| — | — | None recorded. |"]));
  return rows.join("\n") + "\n";
}

export async function writeReleaseMatrix(root = DEFAULT_QA_ROOT, version, resultPaths) {
  const safeVersion = validateVersion(version);
  if (!Array.isArray(resultPaths) || resultPaths.length === 0) {
    throw new Error("matrix requires at least one results file");
  }
  const catalogue = await loadValidatedCatalogue(root);
  const parsedResults = [];
  const targets = new Set();
  for (const path of resultPaths) {
    const envelope = await readJson(resolve(path), basename(path));
    const errors = validateResultEnvelope(envelope, catalogue, basename(path));
    throwIfErrors(errors, "Results validation failed");
    for (const row of envelope.results) {
      const key = [row.id, row.platform, row.surface].join("|");
      if (targets.has(key)) {
        throw new Error("results files overlap target " + key + "; select one authoritative row per target");
      }
      targets.add(key);
    }
    parsedResults.push({ envelope });
  }
  const outputPath = resolve(root, "matrix", safeVersion + ".md");
  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, renderMatrix(safeVersion, catalogue, parsedResults), "utf8");
  return outputPath;
}

function parseListOptions(argumentsList) {
  const filters = {};
  const names = new Map([
    ["--id", "id"],
    ["--tier", "tier"],
    ["--surface", "surface"],
    ["--platform", "platform"],
    ["--area", "area"]
  ]);
  for (let index = 0; index < argumentsList.length; index += 1) {
    const key = names.get(argumentsList[index]);
    const value = argumentsList[index + 1];
    if (!key || !value || value.startsWith("--")) {
      throw new Error("invalid list option " + argumentsList[index]);
    }
    if (Object.hasOwn(filters, key)) {
      throw new Error("duplicate list option " + argumentsList[index]);
    }
    filters[key] = value;
    index += 1;
  }
  if (filters.tier && !TIERS.includes(filters.tier)) {
    throw new Error("unknown tier " + filters.tier);
  }
  if (filters.surface && !SURFACES.includes(filters.surface)) {
    throw new Error("unknown surface " + filters.surface);
  }
  return filters;
}

export async function runCli(argumentsList, root = DEFAULT_QA_ROOT, output = console.log) {
  const [command, ...rest] = argumentsList;
  if (!command || command === "--help" || command === "-h") {
    output(USAGE);
    return;
  }
  if (command === "validate" && rest.length === 0) {
    const summary = await validateCatalogue(root);
    const areaLabel = summary.areas === 1 ? "area" : "areas";
    output(
      "Validated " + summary.total + " cases (" + summary.active + " active, " +
      summary.retired + " retired) across " + summary.areas + " " + areaLabel + "."
    );
    return;
  }
  if (command === "list") {
    const filters = parseListOptions(rest);
    output(renderList(await listCases(root, filters)));
    return;
  }
  if (command === "plan" && rest.length === 1) {
    output(await renderPlan(root, rest[0]));
    return;
  }
  if (command === "matrix" && rest.length >= 2) {
    const path = await writeReleaseMatrix(root, rest[0], rest.slice(1));
    output("Wrote " + path);
    return;
  }
  throw new Error(USAGE);
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  runCli(process.argv.slice(2)).catch(error => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
