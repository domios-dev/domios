import assert from "node:assert/strict";
import test from "node:test";
import { act, useState } from "react";
import { Window } from "happy-dom";
import type { Destination } from "./DestinationPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const window = new Window({ url: "http://localhost/destination-picker" });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLInputElement", "Element", "Node", "Event", "KeyboardEvent", "PointerEvent", "MouseEvent", "MutationObserver"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
}
const workspaces = [{ id: "general", name: "General", protected: true }, { id: "ws-a", name: "Product" }];
const teams = [{ id: "team-a", name: "Docs crew", workspace_id: "ws-a" }, { id: "team-b", name: "Release crew", workspace_id: "ws-a" }];

test("offers only the accepted kinds, searches Teams by name and picks with the keyboard", async () => {
  // React DOM decides at load whether the input event exists, so it loads only after the DOM above does.
  const [{ createRoot }, { DestinationPicker }] = await Promise.all([import("react-dom/client"), import("./DestinationPicker")]);
  function Harness({ kinds, seen }: { kinds?: Destination["kind"][]; seen: Destination[] }) {
    const [value, setValue] = useState<Destination | null>(null);
    return <DestinationPicker kinds={kinds} workspaces={workspaces} teams={teams} value={value} onChange={(next) => { seen.push(next); setValue(next); }} />;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const seen: Destination[] = [];
  await act(async () => root.render(<Harness kinds={["team"]} seen={seen} />));
  const trigger = host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
  assert.equal(trigger.getAttribute("aria-label"), "Destination: Choose a destination", "the closed picker names its empty state");
  await act(async () => trigger.click());
  assert.ok(document.querySelector('[role="dialog"][aria-label="Destination"]'), "the picker opens");
  const segment = (name: string) => document.querySelector<HTMLButtonElement>(`[role="tab"][data-value="${name}"]`);
  assert.equal(segment("general")?.disabled, true, "General waits until the destination accepts it");
  assert.equal(segment("workspace")?.disabled, true, "Workspaces waits too");
  assert.equal(segment("team")?.getAttribute("aria-selected"), "true", "Teams is the open kind");
  const search = document.querySelector<HTMLInputElement>('input[aria-label="Search teams"]');
  assert.ok(search, "Teams has a search field");
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!.call(search, "release");
    search!.dispatchEvent(new window.Event("input", { bubbles: true }) as unknown as Event);
    search!.dispatchEvent(new window.Event("change", { bubbles: true }) as unknown as Event);
  });
  const options = Array.from(document.querySelectorAll<HTMLElement>('[role="option"]'));
  assert.deepEqual(options.map((option) => option.textContent), ["Release crewProduct"], "search narrows Teams to the match, with its workspace hint");
  await act(async () => { options[0].dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true }) as unknown as Event); });
  assert.deepEqual(seen, [{ kind: "team", id: "team-b" }], "Enter picks the focused Team");
  assert.equal(trigger.getAttribute("aria-label"), "Destination: Release crew", "the trigger reads back the choice");
  await act(async () => root.unmount());
  host.remove();
});
