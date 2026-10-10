import assert from "node:assert/strict";
import { act } from "react";
import { Window } from "happy-dom";
import { parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "./blueprintSharing";

async function main() {
  const recipe: BlueprintRecipe = { schema_version: 1, placeholders: { destination: "Destination tab" }, definition: { create: { name: "Fixture watcher", observe: { kind: "tab", id: "{{destination}}" } } } };
  assert.deepEqual(parseBlueprintRecipe(JSON.stringify(recipe)), recipe);
  assert.throws(() => parseBlueprintRecipe(JSON.stringify({ ...recipe, schema_version: 99 })), /schema version/);
  assert.throws(() => parseBlueprintRecipe(JSON.stringify({ ...recipe, placeholders: { destination: 1 } })), /placeholder/);

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
    export: async (...args) => { exports.push(args); return recipe; },
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
  assert.deepEqual(saved, [recipe]);

  let created = 0;
  await act(async () => root.render(<BlueprintImportForm creators={[{ id: "scratch-creator", label: "Scratch Team" }]} onCreated={() => { created++; }} onCancel={() => undefined} api={api} />));
  const fileInput = host.querySelector<HTMLInputElement>('input[type="file"]');
  assert(fileInput);
  Object.defineProperty(fileInput, "files", { configurable: true, value: [{ size: 100, text: async () => JSON.stringify(recipe) }] });
  await act(async () => fileInput.dispatchEvent(new Event("change", { bubbles: true })));
  const field = host.querySelector<HTMLInputElement>('input:not([type="file"])');
  assert(field, "placeholder field must be presented");
  assert(host.querySelector(`label[for="${field.id}"]`));
  await act(async () => {
    Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, "value")?.set?.call(field, "scratch-target");
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => button("Create stopped Blueprint").click());
  assert.deepEqual(imports, [["scratch-creator", recipe, ["destination=scratch-target"]]]);
  assert.equal(created, 1);

  await act(async () => root.unmount());
  host.remove();
  win.happyDOM.abort();
  console.log("blueprintSharing: passed");
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
