import assert from "node:assert/strict";
import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { BlueprintSolutionsApi } from "./blueprintSolutions";

async function main() {
  const win = new Window({ url: "http://localhost" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLInputElement", "SVGElement", "Element", "Node", "Event", "MouseEvent", "KeyboardEvent", "CustomEvent"]) {
    Object.defineProperty(globalThis, key, { configurable: true, value: (win as unknown as Record<string, unknown>)[key] });
  }
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const [{ SavedSolutions }, { SaveSolutionForm }] = await Promise.all([import("./SavedSolutions"), import("./SaveSolutionForm")]);
  const container = document.createElement("div"); document.body.append(container);
  const root = createRoot(container);
  let finishSave!: (value: { id: string; status: "saved" }) => void;
  let archived = "";
  let saved = false;
  const api: BlueprintSolutionsApi = {
    list: async () => [{ id: "recipe", when_to_use: "Watch failing jobs", how: "Poll CI", uses: 7, successes: 6, last_used: 1 }],
    save: () => new Promise((resolve) => { finishSave = resolve; }),
    archive: async (workspace, id) => { archived = `${workspace}/${id}`; },
  };
  const render = (node: ReactElement) => act(async () => { root.render(node); });
  await render(<SaveSolutionForm scope={{ workspace_id: "w", team_id: "t", id: "job" }} api={api} onSaved={() => { saved = true; }} onCancel={() => {}} />);
  const input = container.querySelector("input")!;
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, "value")!.set!;
    setter.call(input, "Watch failing jobs");
    input.dispatchEvent(new win.Event("input", { bubbles: true }) as unknown as Event);
    input.dispatchEvent(new win.Event("change", { bubbles: true }) as unknown as Event);
  });
  await act(async () => { container.querySelector("form")!.dispatchEvent(new win.Event("submit", { bubbles: true, cancelable: true }) as unknown as Event); });
  assert.equal(saved, false, "queue admission must not report persisted success");
  await act(async () => { finishSave({ id: "recipe", status: "saved" }); });
  assert.equal(saved, true);
  await render(<SavedSolutions sources={["w"]} revision={0} api={api} />);
  assert.match(container.textContent!, /7 uses.*6 successes/);
  assert.match(container.textContent!, /Watch failing jobs/);
  await act(async () => { (container.querySelector('button[aria-label="Actions for Watch failing jobs"]') as HTMLButtonElement).click(); });
  const archive = [...document.querySelectorAll("button")].find((button) => button.textContent?.includes("Archive"));
  assert.ok(archive);
  await act(async () => { archive.click(); });
  assert.equal(archived, "w/recipe");
  assert.doesNotMatch(container.textContent!, /Watch failing jobs/);
  await act(async () => root.unmount());
  win.happyDOM.cancelAsync();
  console.log("SavedSolutions: save completion, listing and archive passed");
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
