// WHAT: The change-scoped journey selector and the runner's recorded selection.
// WHY: A release runs exactly the journeys its diff touches; skipped and waived journeys must stay visible.
// ORACLE: Pure inputs (area map, changed paths, verbatim flags) map to exact selections or refusals.
import assert from "node:assert/strict";
import test from "node:test";
import {
  AREA_UNTOUCHED, emptyFlags, loadAreas, resolveSelection, scanSelectionFlags, selectJourneys, selectionErrors,
  selectorOutput, staleAreas,
} from "./selection.mjs";

const areas = { schema: 1, journeys: {
  J1: { requires: [], paths: ["src/spawn/", "plugins/provider-"] },
  J2: { requires: ["J3"], paths: ["src/team/"] },
  J3: { requires: [], paths: ["src/plan/"] },
  J4: { requires: [], paths: ["src/startup/", "src/session.rs"] },
} };
const statuses = (journeys) => Object.fromEntries(Object.entries(journeys).map(([id, entry]) => [id, entry.status]));
const flags = (extra) => ({ ...emptyFlags(), ...extra, exclude_providers: extra?.exclude_providers ?? [] });
const selector = (paths) => selectorOutput(areas, { base: "b".repeat(40), head: "h".repeat(40), paths });

test("changed paths select the journeys whose areas they touch, with a reason per path", () => {
  const { journeys, unmatched_paths } = selectJourneys(areas, ["plugins/provider-codex/x.js", "src\\session.rs", "docs/a.md"]);
  assert.deepEqual(statuses(journeys), { J1: "run", J2: "not_run", J3: "not_run", J4: "run" });
  assert.deepEqual(journeys.J1.reasons, ["plugins/provider-codex/x.js (area plugins/provider-)"]);
  assert.deepEqual(journeys.J4.reasons, ["src/session.rs (area src/session.rs)"]);
  assert.deepEqual(journeys.J2.reasons, [AREA_UNTOUCHED]);
  assert.deepEqual(unmatched_paths, ["docs/a.md"]);
});

test("unknown paths select nothing: there is no run-everything fallback", () => {
  const { journeys, unmatched_paths } = selectJourneys(areas, ["README.md", "src/other/x.rs"]);
  assert.deepEqual(statuses(journeys), { J1: "not_run", J2: "not_run", J3: "not_run", J4: "not_run" });
  assert.equal(unmatched_paths.length, 2);
  assert.deepEqual(statuses(selectJourneys(areas, []).journeys), { J1: "not_run", J2: "not_run", J3: "not_run", J4: "not_run" });
});

test("a selected journey pulls in the journeys it requires, with that reason", () => {
  const { journeys } = selectJourneys(areas, ["src/team/dialog.tsx"]);
  assert.deepEqual(statuses(journeys), { J1: "not_run", J2: "run", J3: "run", J4: "not_run" });
  assert.deepEqual(journeys.J3.reasons, ["required by J2"]);
});

test("stale area prefixes are reported against the tracked files", () => {
  assert.deepEqual(staleAreas(areas, ["src/spawn/a.rs", "plugins/provider-claude/m.json", "src/team/x", "src/plan/y", "src/startup/z"]), ["J4: src/session.rs"]);
});

test("the shipped area map loads and declares J1-J4", () => {
  assert.deepEqual(Object.keys(loadAreas().journeys), ["J1", "J2", "J3", "J4"]);
});

test("no flags run the full suite; --journeys runs only the listed journeys and records the rest as not run", () => {
  assert.deepEqual(statuses(resolveSelection(areas, flags()).journeys), { J1: "run", J2: "run", J3: "run", J4: "run" });
  const operator = resolveSelection(areas, flags({ journeys: "J4" }));
  assert.equal(operator.source, "operator");
  assert.deepEqual(statuses(operator.journeys), { J1: "not_run", J2: "not_run", J3: "not_run", J4: "run" });
  assert.deepEqual(statuses(resolveSelection(areas, flags({ journeys: "J2" })).journeys), { J1: "not_run", J2: "run", J3: "run", J4: "not_run" });
});

test("selector output drives the run; dropping a selected journey needs the owner waiver and is recorded as waived", () => {
  const output = selector(["src/spawn/a.rs", "src/startup/b.rs"]);
  const plain = resolveSelection(areas, flags({ selection: "sel.json" }), output);
  assert.deepEqual(statuses(plain.journeys), { J1: "run", J2: "not_run", J3: "not_run", J4: "run" });
  assert.deepEqual(plain.journeys.J2.reasons, [AREA_UNTOUCHED]);
  assert.throws(() => resolveSelection(areas, flags({ selection: "sel.json", journeys: "J4" }), output), /--waiver .* drop selected J1/);
  const waived = resolveSelection(areas, flags({ selection: "sel.json", journeys: "J4", waiver: "owner: J1 covered by canary" }), output);
  assert.deepEqual(waived.journeys.J1, { status: "waived", reasons: ["src/spawn/a.rs (area src/spawn/)"], waiver: "owner: J1 covered by canary" });
  assert.deepEqual(resolveSelection(areas, flags({ selection: "sel.json", journeys: "J1,J3,J4" }), output).journeys.J3, { status: "run", reasons: ["added by --journeys"] });
});

test("provider exclusion is explicit, needs a waiver and is recorded verbatim", () => {
  assert.throws(() => resolveSelection(areas, flags({ exclude_providers: ["acp"] })), /--waiver .* exclude provider acp/);
  const recorded = resolveSelection(areas, flags({ exclude_providers: ["acp"], waiver: "owner: ACP | gemini quota" }));
  assert.deepEqual(recorded.excluded_providers, [{ id: "acp", waiver: "owner: ACP | gemini quota" }]);
  assert.equal(recorded.flags.waiver, "owner: ACP | gemini quota");
  assert.deepEqual(scanSelectionFlags(["--out", "/o", "--exclude-provider", "acp", "--waiver", "why", "--model", "a=b"]),
    { journeys: null, selection: null, exclude_providers: ["acp"], waiver: "why" });
});

test("invalid selections refuse before any run", () => {
  const output = selector(["src/team/a.tsx"]);
  for (const [given, sel, message] of [
    [flags({ journeys: "J9" }), null, /unknown journey J9/],
    [flags({ journeys: "J1,J1" }), null, /repeats/],
    [flags({ waiver: "unused" }), null, /nothing selected is dropped/],
    [flags({ exclude_providers: ["acp", "acp"], waiver: "w" }), null, /repeats/],
    [flags({ selection: "sel.json" }), selector(["README.md"]), /no journey selected/],
    [flags({ selection: "sel.json", journeys: "J2", waiver: "w" }), output, /J2 requires J3, which is waived/],
    [flags({ selection: "sel.json" }), { ...output, journeys: { ...output.journeys, J4: { status: "run", reasons: ["edited"] } } }, /does not match areas\.json/],
    [flags({ selection: "sel.json" }), null, /needs the selector output/],
  ]) assert.throws(() => resolveSelection(areas, given, sel), message);
});

test("a recorded selection is valid only when its flags and selector output reproduce it", () => {
  const recorded = resolveSelection(areas, flags({ selection: "sel.json", journeys: "J4", waiver: "w" }), selector(["src/spawn/a.rs", "src/startup/b.rs"]));
  assert.deepEqual(selectionErrors(areas, JSON.parse(JSON.stringify(recorded))), []);
  const passed = structuredClone(recorded);
  passed.journeys.J1 = { status: "run", reasons: ["edited"] };
  assert.match(selectionErrors(areas, passed).join(), /differs/);
  const rewaived = structuredClone(recorded);
  rewaived.flags.waiver = "another";
  assert.match(selectionErrors(areas, rewaived).join(), /differs/);
  assert.deepEqual(selectionErrors(areas, undefined), ["results.json: selection missing"]);
});
