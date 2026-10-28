import assert from "node:assert/strict";
import test from "node:test";
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { DestinationPicker, type Destination } from "./DestinationPicker";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const window = new Window({ url: "http://localhost/destination-picker" });
for (const key of ["window", "document", "navigator", "HTMLElement", "HTMLInputElement", "Element", "Node", "Event", "KeyboardEvent", "PointerEvent", "MouseEvent", "MutationObserver"] as const) {
  Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
}

const workspaces = [{ id: "general", name: "General", protected: true }, { id: "ws-a", name: "Product" }];
const teams = [{ id: "team-a", name: "Docs crew", workspace_id: "ws-a" }, { id: "team-b", name: "Release crew", workspace_id: "ws-a" }];

function Harness({ kinds, seen }: { kinds?: Destination["kind"][]; seen: Destination[] }) {
  const [value, setValue] = useState<Destination | null>(null);
  return <DestinationPicker kinds={kinds} workspaces={workspaces} teams={teams} value={value} onChange={(next) => { seen.push(next); setValue(next); }} />;
}

test("offers only the accepted kinds, searches Teams by name and picks with the keyboard", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const seen: Destination[] = [];
  await act(async () => root.render(<Harness kinds={["team"]} seen={seen} />));
  const trigger = host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
  assert.equal(trigger.getAttribute("aria-label"), "Destination: Choose a destination");
  await act(async () => trigger.click());
  const segment = (name: string) => Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === name)!;
  assert.equal(segment("General").disabled, true, "General waits until the destination accepts it");
  assert.equal(segment("Workspaces").disabled, true);
  const search = document.querySelector<HTMLInputElement>('input[aria-label="Search teams"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")!.set!.call(search, "release");
    search.dispatchEvent(new window.Event("input", { bubbles: true }) as unknown as Event);
  });
  const options = Array.from(document.querySelectorAll<HTMLElement>('[role="option"]'));
  assert.deepEqual(options.map((option) => option.textContent?.startsWith("Release crew")), [true]);
  await act(async () => { options[0].dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true }) as unknown as Event); });
  assert.deepEqual(seen, [{ kind: "team", id: "team-b" }]);
  assert.equal(trigger.getAttribute("aria-label"), "Destination: Release crew");
  await act(async () => root.unmount());
  host.remove();
});
