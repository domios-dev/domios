import assert from "node:assert/strict";
import { act } from "react";
import { Window } from "happy-dom";
import { parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "./blueprintSharing";

async function main() {
  const recipe: BlueprintRecipe = { schema_version: 1, name: "Fixture watcher", inputs: { destination: { type: "string", description: "Destination tab" } }, watch: { observe: "tab:{{destination}}", question: "Ready?", buckets: ["done"] } };
  const source = "schema_version: 1\nname: Fixture watcher\ninputs:\n  destination: {type: string, description: Destination tab}\nwatch:\n  observe: 'tab:{{destination}}'\n  question: Ready?\n  buckets: [done]\n";
  assert.deepEqual(parseBlueprintRecipe(source), recipe);
  assert.throws(() => parseBlueprintRecipe("schema_version: 1\nname: First\nname: Second\nwatch: {}"), /unique|duplicate/i);
  assert.deepEqual(parseBlueprintRecipe(JSON.stringify(recipe)), recipe);
  assert.throws(() => parseBlueprintRecipe(JSON.stringify({ ...recipe, schema_version: 99 })), /schema version/);
  assert.throws(() => parseBlueprintRecipe(JSON.stringify({ ...recipe, inputs: { destination: 1 } })), /input/);

  const win = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "HTMLSelectElement", "Element", "Node", "Event", "MouseEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const [{ BlueprintExportButton }, { BlueprintImportForm }] = await Promise.all([import("./BlueprintExportButton"), import("../Jobs/BlueprintImportForm")]);
  const { createRoot } = await import("react-dom/client");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const exports: unknown[] = [];
  const saved: unknown[] = [];
  const imports: unknown[] = [];
  const api: BlueprintSharingApi = {
    export: async (...args) => { exports.push(args); return { name: "Fixture watcher", yaml: source }; },
    save: (value) => { saved.push(value); },
    import: async (...args) => { imports.push(args); return { id: "created", machine: { stopped: true } }; },
  };
  const button = (name: string) => {
    const element = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === name);
    assert(element, `missing button ${name}`);
    return element;
  };
  await act(async () => root.render(<BlueprintExportButton workspaceId="workspace" teamId="team" id="watcher" api={api} />));
  await act(async () => button("Export Blueprint").click());
  assert.deepEqual(exports, [["workspace", "team", "watcher"]]);
  assert.deepEqual(saved, [{ name: "Fixture watcher", yaml: source }]);

  let created = 0;
  await act(async () => root.render(<BlueprintImportForm creators={[{ id: "scratch-creator", label: "Scratch Team" }]} onCreated={() => { created++; }} onCancel={() => undefined} api={api} />));
  const fileInput = host.querySelector<HTMLInputElement>('input[type="file"]');
  assert(fileInput);
  assert(host.textContent?.includes("No file chosen"), "the styled picker speaks English before a choice");
  Object.defineProperty(fileInput, "files", { configurable: true, value: [{ name: "watcher.yaml", size: 100, text: async () => source }] });
  await act(async () => fileInput.dispatchEvent(new Event("change", { bubbles: true })));
  assert(host.textContent?.includes("watcher.yaml"), "the picker shows the chosen file's name");
  const field = host.querySelector<HTMLInputElement>('input:not([type="file"])');
  assert(field, "placeholder field must be presented");
  assert(host.querySelector(`label[for="${field.id}"]`));
  await act(async () => {
    Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, "value")?.set?.call(field, "scratch-target");
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => button("Import stopped Blueprint").click());
  assert.deepEqual(imports, [["scratch-creator", source, ["destination=scratch-target"]]]);
  assert.equal(created, 1);

  await act(async () => root.unmount());
  host.remove();
  win.happyDOM.abort();
  console.log("blueprintSharing: passed");
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
