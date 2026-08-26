// Run: npx tsx src/components/Sidebar/TeamUnit.a11y.test.tsx

import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const [{ act }, { createRoot }, { TeamUnit }] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("./TeamUnit"),
  ]);

  function assert(condition: unknown, message: string): asserts condition {
    if (!condition) throw new Error(message);
  }

  async function render(name: string | null) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(
        <TeamUnit
          team={{ id: "team-1", name, lifecycle: "active", created_at: 0, workspace_id: "ws" }}
          plan={null}
          tabs={{}}
          managers={<div />}
          managerCount={1}
          workers={<div />}
          workerCount={2}
          workingWorkerCount={0}
          terminals={<div />}
          terminalCount={1}
          crons={<div />}
          cronCount={1}
          providers={[]}
          onOpenEffort={() => {}}
          onCloseTeam={() => {}}
        />,
      );
    });
    return { host, root };
  }

  function labels(host: HTMLElement): string[] {
    return Array.from(host.querySelectorAll("[aria-label]")).map(
      (element) => element.getAttribute("aria-label") ?? "",
    );
  }

  // The team name is supplied by the caller and must come back out as the
  // region name — never a fixed string baked into the component.
  for (const teamName of ["Release train", "pw3 accessibility"]) {
    const { host, root } = await render(teamName);
    const found = labels(host);
    const section = host.querySelector('[data-testid="team-unit"]');
    assert(section?.getAttribute("aria-label") === teamName, `team region is named "${teamName}"`);
    for (const suffix of ["managers", "workers", "utility shells", "scheduled jobs"]) {
      assert(found.includes(`${teamName} ${suffix}`), `${suffix} group carries the team name`);
    }
    await act(async () => root.unmount());
    host.remove();
  }

  {
    const { host, root } = await render(null);
    const section = host.querySelector('[data-testid="team-unit"]');
    assert(section?.getAttribute("aria-label") === "Team", "an unnamed team still gets a stable name");
    await act(async () => root.unmount());
    host.remove();
  }

  {
    const { host, root } = await render("Release train");
    const groups = Array.from(host.querySelectorAll('[role="group"][aria-label^="Release train"]'))
      .map((element) => element.getAttribute("aria-label"));
    assert(
      groups.join("|") === [
        "Release train managers",
        "Release train workers",
        "Release train utility shells",
        "Release train scheduled jobs",
      ].join("|"),
      `member groups follow visual order, so focus does too: ${groups.join("|")}`,
    );
    await act(async () => root.unmount());
    host.remove();
  }

  console.log("TeamUnit a11y surface: ok");
}

void main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
