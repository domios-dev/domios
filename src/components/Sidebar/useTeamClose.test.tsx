import assert from "node:assert/strict";
import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

async function main() {
  const [{ act, useState }, { createRoot }, { useTeamClose }, { GroupCloseConfirmDialog }, bridge] = await Promise.all([
    import("react"), import("react-dom/client"), import("./useTeamClose"), import("./GroupCloseConfirmDialog"), import("../../lib/bridge"),
  ]);
  const notices: Array<{ severity: string; body: string | null; local_actions?: { label: string }[] }> = [];
  const notify = (notice: typeof notices[number]) => { notices.push(notice); };
  const empty = { removed: [], kept: [], orphan_branches: [] };
  type Inventory = import("../../lib/teamActions").TeamCloseInventory;
  let settle!: (inventory: Inventory) => void;
  let reject!: (error: Error) => void;
  let host!: HTMLDivElement;
  const close = () => {
    return new Promise<Inventory>((resolve, fail) => { settle = resolve; reject = fail; });
  };
  function Harness() {
    const [open, setOpen] = useState(true);
    const { closingTeamIds, closeTeam } = useTeamClose(close, notify);
    return <>
      {closingTeamIds.has("team") && <span role="status" aria-label="Closing Team" />}
      {open && <GroupCloseConfirmDialog mode="team" orchestratorTitle="Team" members={[]} onCancel={() => {}} onConfirm={() => {
        void closeTeam("team", "ws", () => setOpen(false));
      }} />}
    </>;
  }
  async function mount() {
    host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => root.render(<Harness />));
    const button = [...host.querySelectorAll("button")].find(button => button.textContent === "Close Team")!;
    await act(async () => button.click());
    assert.equal(host.querySelector('[role="dialog"]'), null);
    assert(host.querySelector('[role="status"][aria-label="Closing Team"]'));
    return root;
  }
  let root = await mount();
  await act(async () => settle(empty));
  assert.equal(notices.length, 0);
  assert.equal(host.querySelector('[role="status"]'), null);
  await act(async () => root.unmount());
  root = await mount();
  await act(async () => settle({ ...empty, kept: [{ path: "C:/lanes/preserved", branch: "lane/work", reason: "dirty" }] }));
  assert.equal(notices[0].severity, "Info");
  assert.equal(notices[0].body, "C:/lanes/preserved");
  await act(async () => root.unmount());
  notices.length = 0;
  const { configureOrchestrationTransport } = await import("../../lib/transport/orchestrationRuntime");
  configureOrchestrationTransport({
    json: async <T,>() => ({ blueprints: [] }) as T,
    localJson: async <T,>() => ({}) as T,
    upload: async <T,>() => ({}) as T,
    subscribe: () => () => {}, currentToken: () => null, origin: () => "http://localhost/",
  });
  bridge.__setInvokeForTest(async <T,>(command: string) => {
    if (command === "mux_get_state") return { workspaces: [], tabs: {}, teams: [] } as T;
    if (command === "cron_list") return [] as T;
    return { blueprints: [] } as T;
  });
  root = await mount();
  await act(async () => reject(new Error("bookkeeping failed after close")));
  assert.equal(notices.length, 0);
  await act(async () => root.unmount());
  bridge.__setInvokeForTest(async <T,>(command: string) => {
    if (command === "mux_get_state") return { tabs: { surviving: { id: "surviving", title: "Worker", team_id: "team" }, foreign: { id: "foreign", title: "Foreign", team_id: "other" } } } as T;
    return [] as T;
  });
  root = await mount();
  await act(async () => reject(new Error("partial teardown")));
  assert.equal(notices[0].severity, "Error");
  assert.equal(notices[0].body, "Tab: Worker\npartial teardown");
  assert.equal(notices[0].local_actions?.[0].label, "Retry");
  await act(async () => root.unmount());
  bridge.__setInvokeForTest(null);
}
void main();
