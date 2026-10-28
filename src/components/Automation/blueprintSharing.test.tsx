import assert from "node:assert/strict";
import { act } from "react";
import { Window } from "happy-dom";
import { parseBlueprintRecipe, type BlueprintRecipe, type BlueprintSharingApi } from "./blueprintSharing";

async function main() {
  const recipe: BlueprintRecipe = { name: "Fixture watcher", inputs: { destination: { type: "string", description: "Destination tab" } } };
  const source = "schema_version: 1\nname: Fixture watcher\ninputs:\n  destination: {type: string, description: Destination tab}\nwatch:\n  observe: 'tab:{{destination}}'\n  question: Ready?\n  buckets: [done]\n";
  assert.deepEqual(parseBlueprintRecipe(source), recipe);
  assert.throws(() => parseBlueprintRecipe("schema_version: 1\nname: First\nname: Second\nwatch: {}"), /unique|duplicate/i);
  assert.throws(() => parseBlueprintRecipe("- not\n- a mapping"), /Invalid Blueprint document/);
  // The schema is the runtime's to judge: a native v2 recipe reads, and its bytes go to the import unchanged.
  assert.deepEqual(parseBlueprintRecipe("schema_version: 2\nname: Native\nsteps: []\n"), { name: "Native" });
  assert.deepEqual(parseBlueprintRecipe(JSON.stringify({ name: "Odd", inputs: { destination: 1 } })), { name: "Odd" }, "an input the form cannot bind is left to the runtime");

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
  // Opened from General with a General tab and a Team tab: General comes first, and the import names it.
  await act(async () => root.render(<BlueprintImportForm creators={[{ id: "team-creator", label: "Team tab", scope: { kind: "team", id: "team" } }, { id: "scratch-creator", label: "Main", scope: { kind: "general" } }]}
    workspaces={[{ id: "general", name: "General", protected: true }]} teams={[{ id: "team", name: "Scratch Team" }]} initialDestination={{ kind: "general" }}
    onCreated={() => { created++; }} onCancel={() => undefined} api={api} />));
  assert.equal(host.querySelector('button[aria-haspopup="dialog"]')?.getAttribute("aria-label"), "Destination: General", "opened from General, General is preselected");
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
  assert.deepEqual(imports, [["scratch-creator", source, ["destination=scratch-target"], "general"]]);
  assert.equal(created, 1);

  await act(async () => root.unmount());
  host.remove();
  win.happyDOM.abort();
  console.log("blueprintSharing: passed");
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
