import assert from "node:assert/strict";
import { act, type ReactElement } from "react";
import { Window } from "happy-dom";
import { stringify } from "yaml";
import type { BlueprintSolutionsApi } from "./blueprintSolutions";
import type { CatalogEntry } from "./solutionCatalog";
import { catalogEntry, filterCatalog, NO_CATALOG_FILTER, solutionKind } from "./solutionCatalog";

const callable = { schema_version: 2, kind: "pipeline", name: "add-seventeen", steps: [{ id: "add", code: { lang: "python", src: "def main(v): return v + 17" }, in: { type: "integer" }, out: { type: "integer" } }], result: { from: "add.out" } };
const watch = { schema_version: 2, kind: "watch", name: "ci-watch", steps: [{ id: "source", uses: "probe", with: { command: { lit: "gh run list" } } }, { id: "decide", uses: "ask", buckets: ["failing"], routes: { failing: "notify" } }, { id: "notify", uses: "notify" }] };
const pipeline = { ...callable, name: "readiness", trigger: { type: "schedule", every: { lit: "1h" } } };

async function main() {
  assert.equal(solutionKind(callable as never), "call");
  assert.equal(solutionKind(watch as never), "watch");
  assert.equal(solutionKind(pipeline as never), "pipeline");
  const solution = (id: string, when: string, workspaceId = "w") => ({ id, workspaceId, when_to_use: when, how: null, uses: 1, successes: 1, last_used: 1 });
  const catalog = [
    catalogEntry(solution("a", "Increment a typed integer"), { title: "add-seventeen", body: stringify(callable), tags: ["blueprint-library", "math"] }),
    catalogEntry(solution("b", "Watch failing CI runs"), { title: "ci-watch", body: stringify(watch), tags: ["blueprint-library", "ci"] }),
    catalogEntry(solution("c", "Score release readiness", "x"), { title: "readiness", body: stringify(pipeline), tags: ["blueprint-library"] }),
    catalogEntry(solution("d", "Broken entry"), new Error("not found")),
  ];
  assert.deepEqual(catalog[0].tags, ["math"], "the library marker is not a catalogue tag");
  assert.equal(catalog[3].error, "not found");
  const ids = (entries: readonly CatalogEntry[]) => entries.map((entry) => entry.solution.id);
  assert.deepEqual(ids(filterCatalog(catalog, { ...NO_CATALOG_FILTER, query: "failing ci" })), ["b"], "every typed word must match");
  assert.deepEqual(ids(filterCatalog(catalog, { ...NO_CATALOG_FILTER, kind: "call" })), ["a"]);
  assert.deepEqual(ids(filterCatalog(catalog, { ...NO_CATALOG_FILTER, tag: "ci" })), ["b"]);
  assert.deepEqual(ids(filterCatalog(catalog, { ...NO_CATALOG_FILTER, workspaceId: "x" })), ["c"]);
  assert.deepEqual(ids(filterCatalog(catalog, { ...NO_CATALOG_FILTER, query: "release gate" }, ["c", "a"])), ["c", "a"], "a semantic ranking keeps meaning matches in rank order");

  const win = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent", "MutationObserver"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const [{ createRoot }, { SavedSolutions }, { SaveSolutionForm }, { SavedSolutionView }] = await Promise.all([import("react-dom/client"), import("./SavedSolutions"), import("./SaveSolutionForm"), import("./SavedSolutionView")]);
  const container = document.createElement("div"); document.body.append(container);
  const root = createRoot(container);
  let finishSave!: (value: { id: string; status: "saved" }) => void;
  const archived: string[] = [];
  const searches: string[] = [];
  let saved = false;
  const api: BlueprintSolutionsApi = {
    list: async (workspace) => workspace === "w" ? [
      { id: "recipe", when_to_use: "Watch failing jobs", how: "Poll CI", uses: 7, successes: 6, last_used: 1 },
      { id: "helper", when_to_use: "Increment a typed integer", how: null, uses: 0, successes: 0, last_used: 0 },
    ] : [],
    save: () => new Promise((resolve) => { finishSave = resolve; }),
    archive: async (workspace, id) => { archived.push(`${workspace}/${id}`); },
    document: async (id) => id === "recipe" ? { title: "ci-watch", body: stringify(watch), tags: ["blueprint-library", "ci"] } : { title: "add-seventeen", body: stringify(callable), tags: ["blueprint-library", "math"] },
    search: async (workspace, query) => { searches.push(`${workspace}:${query}`); return ["helper"]; },
  };
  const render = (node: ReactElement) => act(async () => { root.render(node); });
  const flush = async () => { for (let index = 0; index < 6; index++) await act(async () => { await Promise.resolve(); }); };
  const type = async (input: HTMLInputElement, value: string) => act(async () => {
    Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new win.Event("input", { bubbles: true }) as unknown as Event);
    input.dispatchEvent(new win.Event("change", { bubbles: true }) as unknown as Event);
  });
  const rows = () => [...container.querySelectorAll("button[data-job-key]")].map((button) => button.getAttribute("data-job-key"));

  await render(<SaveSolutionForm scope={{ workspace_id: "w", team_id: "t", id: "job" }} api={api} onSaved={() => { saved = true; }} onCancel={() => {}} />);
  const input = container.querySelector("input")!;
  await type(input, "Watch failing jobs");
  await act(async () => { container.querySelector("form")!.dispatchEvent(new win.Event("submit", { bubbles: true, cancelable: true }) as unknown as Event); });
  assert.equal(saved, false, "queue admission must not report persisted success");
  await act(async () => { finishSave({ id: "recipe", status: "saved" }); });
  assert.equal(saved, true);

  const opened: CatalogEntry[] = [];
  await render(<SavedSolutions sources={["w"]} revision={0} api={api} onOpen={(entry) => opened.push(entry)} />);
  await flush();
  assert.match(container.textContent!, /Watch · 7 uses · 6 successes/);
  assert.deepEqual(rows(), ["solution:recipe", "solution:helper"]);
  const search = container.querySelector<HTMLInputElement>('input[aria-label="Search saved solutions"]')!;
  await type(search, "integer");
  assert.deepEqual(rows(), ["solution:helper"], "typing filters the catalogue at once");
  await type(search, "add numbers");
  assert.deepEqual(rows(), [], "no text match before semantic ranking");
  await act(async () => { container.querySelector('form[role="search"]')!.dispatchEvent(new win.Event("submit", { bubbles: true, cancelable: true }) as unknown as Event); });
  await flush();
  assert.deepEqual(searches, ["w:add numbers"]);
  assert.deepEqual(rows(), ["solution:helper"], "Enter ranks by meaning through memory search");
  await type(search, "");
  assert(container.querySelector('button[aria-label="Filter by kind"]'));
  assert(container.querySelector('button[aria-label="Filter by tag"]'));
  await act(async () => { (container.querySelector('button[data-job-key="solution:helper"]') as HTMLButtonElement).click(); });
  assert.deepEqual(opened.map((entry) => entry.solution.id), ["helper"], "a row opens its solution");
  await act(async () => { container.querySelector('button[aria-label="Actions for Watch failing jobs"]')!.dispatchEvent(new win.PointerEvent("pointerdown", { bubbles: true, button: 0 }) as unknown as Event); });
  const remove = [...document.querySelectorAll("button")].find((button) => button.textContent?.includes("Delete"));
  assert.ok(remove);
  await act(async () => { remove.click(); });
  assert.deepEqual(archived, ["w/recipe"]);
  assert.deepEqual(rows(), ["solution:helper"]);

  const exported: string[] = [];
  let deleted = 0, back = 0;
  await render(<SavedSolutionView entry={opened[0]} api={api} sharing={{ export: async () => { throw new Error("unused"); }, save: (file: { name: string }) => exported.push(file.name), import: async () => { throw new Error("unused"); } } as never}
    backLabel="All jobs" onBack={() => back++} onDeleted={() => deleted++} />);
  await flush();
  assert(container.querySelector('[aria-label="Node add"]'), "the saved graph renders in the single Blueprint view");
  assert(container.querySelector('[aria-label="Saved solution facts"]')?.textContent?.includes("Increment a typed integer"));
  assert.equal(container.querySelector('[aria-label="Blueprint runs"]'), null, "a saved solution has no runs panel");
  const button = (name: string) => [...container.querySelectorAll("button")].find((entry) => (entry.getAttribute("aria-label") ?? entry.textContent) === name)!;
  await act(async () => button("Export Blueprint").click());
  assert.deepEqual(exported, ["add-seventeen"]);
  await act(async () => button("Delete").click());
  await flush();
  assert.equal(deleted, 1);
  assert.deepEqual(archived, ["w/recipe", "w/helper"]);
  await act(async () => button("Back to All jobs").click());
  assert.equal(back, 1);
  await act(async () => root.unmount());
  win.happyDOM.cancelAsync();
  console.log("SavedSolutions: save completion, catalogue filters, semantic ranking, open, export and delete passed");
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
