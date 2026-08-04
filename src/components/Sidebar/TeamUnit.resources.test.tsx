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
  await act(async () => {
    root.render(
      <TeamUnit
        team={{ id: "resources", name: "Resources", lifecycle: "active", created_at: 0, workspace_id: "ws" }}
        plan={null}
        tabs={{}}
        managers={null}
        managerCount={1}
        workers={null}
        workerCount={1}
        workingWorkerCount={0}
        terminals={<span data-testid="terminal-entry">dev server</span>}
        terminalCount={1}
        crons={<span data-testid="cron-entry">nightly</span>}
        cronCount={1}
        providers={[]}
        onOpenEffort={() => {}}
      />,
    );
  });

  assert(host.querySelector('[data-testid="terminals-section"]'), "terminal section appears when populated");
  assert(host.querySelector('[data-testid="crons-section"]'), "cron section appears when populated");
  assert(!host.querySelector('[data-testid="terminal-entry"]'), "terminal section starts collapsed");
  assert(!host.querySelector('[data-testid="cron-entry"]'), "cron section starts collapsed");
  assert(host.querySelector('[data-testid="terminals-collapse-toggle"]')?.textContent?.includes("1 utility shell"), "collapsed terminal disclosure carries its count inline");
  assert(host.querySelector('[data-testid="crons-collapse-toggle"]')?.textContent?.includes("1 scheduled job"), "collapsed cron disclosure carries its count inline");

  await act(async () => (host.querySelector('[data-testid="terminals-collapse-toggle"]') as HTMLButtonElement).click());
  await act(async () => (host.querySelector('[data-testid="crons-collapse-toggle"]') as HTMLButtonElement).click());
  assert(host.querySelector('[data-testid="terminal-entry"]'), "terminal section expands on demand");
  assert(host.querySelector('[data-testid="cron-entry"]'), "cron section expands on demand");
  assert(host.querySelector('[data-testid="terminals-collapse-toggle"]')?.textContent?.includes("UTILITY SHELLS"), "expanded terminal disclosure becomes a category label");
  assert(host.querySelector('[data-testid="crons-collapse-toggle"]')?.textContent?.includes("SCHEDULED JOBS"), "expanded cron disclosure becomes a category label");
  await act(async () => root.unmount());

  console.log("TeamUnit resource disclosure tests passed");
}

void main();
