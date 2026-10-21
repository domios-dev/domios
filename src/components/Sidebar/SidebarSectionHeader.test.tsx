import assert from "node:assert/strict";
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import type { SidebarSectionKind } from "./sidebarSectionModel";
const window = new Window({ url: "http://localhost/sections" });
for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node"] as const) Object.defineProperty(globalThis, key, { value: key === "window" ? window : (window as unknown as Record<string, unknown>)[key], configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
function Harness({ kind, working }: { kind: SidebarSectionKind; working: number }) {
  const [expanded, setExpanded] = useState(false);
  return <><SidebarSectionHeader kind={kind} count={6} working={working} expanded={expanded} aria-controls="section-members" onClick={() => setExpanded(value => !value)} /><div id="section-members" hidden={!expanded}>Members</div></>;
}
async function main() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  for (const kind of ["managers", "workers", "agents", "utilities", "jobs", "services"] satisfies SidebarSectionKind[]) {
    for (const working of [0, 4]) {
      await act(async () => root.render(<Harness key={`${kind}-${working}`} kind={kind} working={working} />));
      const button = host.querySelector("button")!;
      assert.equal(button.type, "button");
      assert.equal(button.getAttribute("aria-label"), `${kind.toUpperCase()} 6${working ? ", 4 working" : ""}`);
      assert.equal(button.textContent?.includes("working"), working > 0);
      assert.equal(button.getAttribute("aria-expanded"), "false");
      assert.equal(document.getElementById(button.getAttribute("aria-controls")!)?.hidden, true);
      await act(async () => button.click());
      assert.equal(button.getAttribute("aria-expanded"), "true");
      assert.equal(host.querySelector<HTMLDivElement>("#section-members")?.hidden, false);
    }
  }
  await act(async () => root.unmount());host.remove();
  console.log("SidebarSectionHeader: 12 idle/working group toggle cases passed");
}
void main();
