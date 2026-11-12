import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const AREAS_PATH = fileURLToPath(new URL("../areas.json", import.meta.url));
export const AREA_UNTOUCHED = "area untouched";
export const SOURCES = ["full", "operator", "selector"];
export const STATUSES = ["run", "not_run", "waived"];

export class SelectionRefusal extends Error {}
const refuse = (reason) => { throw new SelectionRefusal(reason); };
const isText = (value) => typeof value === "string" && value.trim() !== "";

export function loadAreas(path = AREAS_PATH) {
  const areas = JSON.parse(readFileSync(path, "utf8"));
  if (areas?.schema !== 1 || !areas.journeys || typeof areas.journeys !== "object") throw new Error(`${path}: expected schema 1 with journeys`);
  for (const [id, area] of Object.entries(areas.journeys)) {
    if (!Array.isArray(area.paths) || !area.paths.every(isText)) throw new Error(`${path}: ${id}.paths must be non-empty strings`);
    if (!Array.isArray(area.requires) || !area.requires.every((other) => Object.hasOwn(areas.journeys, other))) throw new Error(`${path}: ${id}.requires names an unknown journey`);
  }
  return areas;
}

export const journeyIds = (areas) => Object.keys(areas.journeys);

const normalize = (path) => String(path).replaceAll("\\", "/").replace(/^\.\//, "");

// Closes a run set over `requires`; a required journey that was waived cannot be satisfied.
function closeOver(areas, journeys) {
  for (let changed = true; changed;) {
    changed = false;
    for (const id of journeyIds(areas)) {
      if (journeys[id].status !== "run") continue;
      for (const needed of areas.journeys[id].requires) {
        const entry = journeys[needed];
        if (entry.status === "waived") refuse(`${id} requires ${needed}, which is waived; waive ${id} too or run ${needed}`);
        if (entry.status === "not_run") { journeys[needed] = { status: "run", reasons: [] }; changed = true; }
        const reason = `required by ${id}`;
        if (!journeys[needed].reasons.includes(reason)) journeys[needed].reasons.push(reason);
      }
    }
  }
  return journeys;
}

// Pure: changed repository paths in, the journeys whose declared areas they touch out. Unknown paths select nothing.
export function selectJourneys(areas, paths) {
  const journeys = {}, unmatched = [];
  const changed = [...new Set((paths ?? []).map(normalize).filter(Boolean))].sort();
  for (const id of journeyIds(areas)) journeys[id] = { status: "not_run", reasons: [] };
  for (const path of changed) {
    let matched = false;
    for (const id of journeyIds(areas)) {
      const prefix = areas.journeys[id].paths.find((entry) => path.startsWith(entry));
      if (!prefix) continue;
      matched = true;
      journeys[id].status = "run";
      journeys[id].reasons.push(`${path} (area ${prefix})`);
    }
    if (!matched) unmatched.push(path);
  }
  closeOver(areas, journeys);
  for (const entry of Object.values(journeys)) if (entry.status === "not_run") entry.reasons = [AREA_UNTOUCHED];
  return { journeys, unmatched_paths: unmatched };
}

// Area prefixes that no tracked file matches: a renamed source would otherwise stop selecting silently.
export function staleAreas(areas, tracked) {
  const files = tracked.map(normalize);
  return journeyIds(areas).flatMap((id) => areas.journeys[id].paths
    .filter((prefix) => !files.some((file) => file.startsWith(prefix))).map((prefix) => `${id}: ${prefix}`));
}

export function selectorOutput(areas, { base, head, paths }) {
  const changed = [...new Set(paths.map(normalize).filter(Boolean))].sort();
  return { schema: 1, base, head, changed_paths: changed, ...selectJourneys(areas, changed) };
}

export function emptyFlags() {
  return { journeys: null, selection: null, exclude_providers: [], waiver: null };
}

// Consumes one runner selection flag; returns false for any other flag.
export function takeSelectionFlag(flags, flag, value) {
  if (!["--journeys", "--selection", "--exclude-provider", "--waiver"].includes(flag)) return false;
  if (!isText(value) || value.startsWith("--")) refuse(`${flag} needs a value`);
  if (flag === "--exclude-provider") flags.exclude_providers.push(value);
  else if (flags[flag.slice(2)] !== null) refuse(`${flag} given twice`);
  else flags[flag.slice(2)] = value;
  return true;
}

export function scanSelectionFlags(argv) {
  const flags = emptyFlags();
  for (let index = 0; index < argv.length; index += 1) if (takeSelectionFlag(flags, argv[index], argv[index + 1])) index += 1;
  return flags;
}

function checkSelector(areas, selector) {
  if (selector?.schema !== 1 || !isText(selector.base) || !isText(selector.head) || !Array.isArray(selector.changed_paths)) {
    refuse("--selection is not selector output (schema 1 with base, head and changed_paths)");
  }
  const expected = selectorOutput(areas, { base: selector.base, head: selector.head, paths: selector.changed_paths });
  if (JSON.stringify(expected) !== JSON.stringify(selector)) refuse("--selection does not match areas.json for its own changed_paths; rerun select.mjs");
}

// Pure: the verbatim runner flags (and the selector output they name) in, the recorded selection out.
export function resolveSelection(areas, flags, selector = null) {
  const ids = journeyIds(areas);
  const listed = flags.journeys === null ? null : flags.journeys.split(",").map((id) => id.trim()).filter(Boolean);
  if (listed) {
    if (!listed.length) refuse("--journeys lists no journey");
    for (const id of listed) if (!ids.includes(id)) refuse(`--journeys names unknown journey ${id}; known: ${ids.join(",")}`);
    if (new Set(listed).size !== listed.length) refuse("--journeys repeats a journey");
  }
  if ((flags.selection === null) !== (selector === null)) refuse("--selection needs the selector output it names");
  if (selector) checkSelector(areas, selector);
  const excluded = flags.exclude_providers;
  if (!excluded.every(isText) || new Set(excluded).size !== excluded.length) refuse("--exclude-provider repeats or is blank");

  const journeys = {};
  for (const id of ids) {
    if (selector) journeys[id] = structuredClone(selector.journeys[id]);
    else if (listed) journeys[id] = listed.includes(id) ? { status: "run", reasons: ["selected by --journeys"] } : { status: "not_run", reasons: ["not in --journeys"] };
    else journeys[id] = { status: "run", reasons: ["full suite: no selection given"] };
    if (selector && listed && !listed.includes(id) && journeys[id].status === "run") journeys[id] = { ...journeys[id], status: "waived", waiver: flags.waiver };
    else if (selector && listed && listed.includes(id) && journeys[id].status === "not_run") journeys[id] = { status: "run", reasons: ["added by --journeys"] };
  }
  closeOver(areas, journeys);
  if (!ids.some((id) => journeys[id].status === "run")) refuse("no journey selected to run; keep the selector output as the not-run record instead of starting the suite");
  const waived = ids.filter((id) => journeys[id].status === "waived");
  const needsWaiver = waived.length > 0 || excluded.length > 0;
  if (needsWaiver && !isText(flags.waiver)) refuse(`--waiver "<owner reason>" is required to ${[...waived.map((id) => `drop selected ${id}`), ...excluded.map((id) => `exclude provider ${id}`)].join(", ")}`);
  if (!needsWaiver && flags.waiver !== null) refuse("--waiver given but nothing selected is dropped and no provider is excluded");
  return {
    source: selector ? "selector" : listed ? "operator" : "full",
    flags: structuredClone(flags),
    selector: selector ? structuredClone(selector) : null,
    journeys,
    excluded_providers: excluded.map((id) => ({ id, waiver: flags.waiver })),
  };
}

export function selectionFromFlags(areas, flags, read = (path) => readFileSync(path, "utf8")) {
  let selector = null;
  if (flags.selection !== null) {
    try { selector = JSON.parse(read(flags.selection)); } catch (error) { refuse(`--selection ${flags.selection}: ${error.message}`); }
  }
  return resolveSelection(areas, flags, selector);
}

export const runs = (selection, id) => selection?.journeys?.[id]?.status === "run";
export const excludedProviders = (selection) => (selection?.excluded_providers ?? []).map((entry) => entry.id);

// A recorded selection is valid only if the same flags and selector output reproduce it exactly.
export function selectionErrors(areas, selection) {
  if (!selection || typeof selection !== "object") return ["results.json: selection missing"];
  if (!SOURCES.includes(selection.source)) return [`results.json: selection.source ${JSON.stringify(selection.source)} is not one of ${SOURCES.join("|")}`];
  try {
    const flags = { ...emptyFlags(), ...selection.flags, exclude_providers: [...(selection.flags?.exclude_providers ?? [])] };
    const expected = resolveSelection(areas, flags, selection.selector ?? null);
    if (JSON.stringify(expected) !== JSON.stringify(selection)) return ["results.json: selection differs from what its recorded flags and selector output resolve to"];
  } catch (error) {
    return [`results.json: selection invalid: ${error.message}`];
  }
  return [];
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, dir] = process.argv.slice(2);
  const path = dir ? join(resolve(dir), "results.json") : "";
  if (command !== "restarts" || !path || !existsSync(path)) {
    console.error("Usage: node evals/core-journeys/lib/selection.mjs restarts <run-dir>\nExit 0: J4 is selected, so the cycle restarts; 1: J4 is not selected; 2: no recorded selection.");
    process.exitCode = 2;
  } else {
    const selection = JSON.parse(readFileSync(path, "utf8")).selection;
    process.exitCode = selection?.journeys ? (runs(selection, "J4") ? 0 : 1) : 2;
  }
}
