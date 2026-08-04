import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "navigator", "HTMLElement", "SVGElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function main() {
  const [{ act }, { createRoot }, { TeamUnit }] = await Promise.all([
    import("react"),
    import("react-dom/client"),
    import("./TeamUnit"),
  ]);
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let closes = 0;
  await act(async () => {
    root.render(
      <TeamUnit
        team={{ id: "team-1", name: "Team", lifecycle: "active", created_at: 0, workspace_id: "ws" }}
        plan={null}
        tabs={{}}
        managers={null}
        managerCount={1}
        workers={null}
        workerCount={0}
        workingWorkerCount={0}
        providers={[]}
        onOpenEffort={() => {}}
        onCloseTeam={() => { closes += 1; }}
      />,
    );
  });
  await act(async () => (host.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  const close = Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
    .find((button) => button.textContent?.includes("Close Team")) as HTMLButtonElement;
  assert(close, "Close Team is enabled only when a close handler exists");
  await act(async () => close.click());
  assert(closes === 1, "Close Team invokes its single handler exactly once");
  await act(async () => root.unmount());

  const managerlessHost = document.createElement("div");
  document.body.appendChild(managerlessHost);
  const managerlessRoot = createRoot(managerlessHost);
  await act(async () => {
    managerlessRoot.render(
      <TeamUnit
        team={{ id: "team-2", name: "Managerless", lifecycle: "forming", created_at: 0, workspace_id: "ws" }}
        plan={null}
        tabs={{}}
        managers={null}
        managerCount={0}
        workers={null}
        workerCount={0}
        workingWorkerCount={0}
        providers={[]}
        onOpenEffort={() => {}}
        onCloseTeam={() => { closes += 1; }}
      />,
    );
  });
  await act(async () => (managerlessHost.querySelector('[data-testid="team-menu-trigger"]') as HTMLButtonElement).click());
  assert(
    Array.from(document.querySelectorAll('[data-testid="team-menu"] button'))
      .some((button) => button.textContent?.includes("Close Team")),
    "managerless Team keeps the human Close Team action",
  );
  assert(closes === 1, "opening a managerless Team menu causes no close invocation");
  await act(async () => managerlessRoot.unmount());
  console.log("TeamUnit close tests passed");
}

void main();
