// Run: npx tsx src/components/Sidebar/TeamUnit.activity.test.tsx

import { readFileSync } from "node:fs";
import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  navigator: window.navigator,
  HTMLElement: window.HTMLElement,
  SVGElement: window.SVGElement,
  Element: window.Element,
  Node: window.Node,
  Event: window.Event,
  localStorage: window.localStorage,
})) Object.defineProperty(globalThis, name, { value, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const [{ act }, { createRoot }, { TeamUnit }] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("./TeamUnit"),
  ]);
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(
    <TeamUnit
      team={{ id: "team-1", name: "Constrained Team Title", lifecycle: "active", created_at: 0, workspace_id: "ws" }}
      plan={null}
      tabs={{}}
      managers={null}
      managerCount={0}
      workers={null}
      workerCount={0}
      workingWorkerCount={1}
      providers={[]}
      onOpenEffort={() => {}}
    />,
  ));

  const title = host.querySelector('[data-testid="team-name"]');
  const working = host.querySelector('[data-testid="team-working-indicator"]');
  const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  if (host.querySelectorAll('[data-testid="team-working-indicator"][role="status"]').length !== 1) {
    throw new Error("working Team exposes exactly one status wrapper");
  }
  if (working?.getAttribute("aria-label") !== `${title?.textContent} has working members`) {
    throw new Error("working Team keeps its accessible name");
  }
  if (!/\.sidebar-team-title-row\s*\{[^}]*display:\s*inline-flex;[^}]*flex-wrap:\s*nowrap;/s.test(styles)) {
    throw new Error("Team title row is an inline-flex nowrap geometry boundary");
  }
  if (!/\.sidebar-team-title-row\s+\.activity-dots\s*\{[^}]*flex:\s*0 0 auto;/s.test(styles)) {
    throw new Error("Team activity dots keep a fixed flex boundary under constrained title width");
  }
  if (!working?.querySelector(".activity-dots--small")) {
    throw new Error("Team activity dots use shared small geometry");
  }
  if (working?.querySelector('[aria-hidden="true"]')?.children.length !== 3) {
    throw new Error("working Team owns exactly three hidden dot children");
  }

  await act(async () => root.unmount());
  console.log("TeamUnit activity tests passed");
}

void main();
