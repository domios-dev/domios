import assert from "node:assert/strict";
import type { Notice } from "../../lib/notices";
import { Window } from "happy-dom";

const window = new Window({ url: "http://localhost/" });
for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event"] as const) {
  Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

async function main() {
  const [{ act, useState }, { createRoot }, { useTeamClose, useTeamClosePreview }, { GroupCloseConfirmDialog }, bridge] = await Promise.all([
    import("react"), import("react-dom/client"), import("./useTeamClose"), import("./GroupCloseConfirmDialog"), import("../../lib/bridge"),
  ]);
  const notices: Notice[] = [];
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
  assert.equal(notices[0].detail, "C:/lanes/preserved");
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
  assert.equal(notices[0].title, "Team cleanup reported an error");
  assert.equal(notices[0].detail, "bookkeeping failed after close");
  assert.equal(notices[0].actions, undefined);
  await act(async () => root.unmount());
  notices.length = 0;
  bridge.__setInvokeForTest(async <T,>(command: string) => {
    if (command === "mux_get_state") return { tabs: { surviving: { id: "surviving", title: "Worker", team_id: "team" }, foreign: { id: "foreign", title: "Foreign", team_id: "other" } } } as T;
    return [] as T;
  });
  root = await mount();
  await act(async () => reject(new Error("partial teardown")));
  assert.equal(notices[0].severity, "Error");
  assert.equal(notices[0].detail, "Tab: Worker\npartial teardown");
  assert.equal(notices[0].actions?.[0].label, "Retry");
  await act(async () => root.unmount());
  bridge.__setInvokeForTest(null);

  const team = { id: "team", name: "Test Team", members: [], crons: ["Test Cron"], blueprints: [], lanes: [] };
  type Preview = { agentTitles: string[]; blueprints: string[]; teams: typeof team[] };
  let resolvePreview!: (result: Preview) => void;
  let signal: AbortSignal | undefined;
  const read = (workspaceId: string, teamId: string | null, requestSignal?: AbortSignal) => {
    assert.equal(workspaceId, "ws");
    assert.equal(teamId, team.id);
    signal = requestSignal;
    return new Promise<Preview>(resolve => { resolvePreview = resolve; });
  };
  let confirmations = 0;
  function PreviewHarness() {
    const { pending, request, dismiss } = useTeamClosePreview(read);
    return <>
      <button onClick={() => { void request({ ...team, workspaceId: "ws" }); }}>Open</button>
      {pending && <GroupCloseConfirmDialog mode="team" orchestratorTitle={pending.name} members={pending.members}
        crons={pending.phase === "ready" ? pending.crons : undefined} loading={pending.phase === "loading"}
        onCancel={dismiss} onConfirm={() => { confirmations++; }} />}
    </>;
  }
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(<PreviewHarness />));
  const click = async (label: string) => {
    const button = [...host.querySelectorAll("button")].find(button => button.textContent?.trim() === label);
    assert(button, label);
    await act(async () => button.click());
  };
  await click("Open");
  assert(host.querySelector('[role="dialog"][aria-label="Close Team"]'));
  assert(host.querySelector('[role="status"][aria-label="Reading Team inventory"]'));
  await click("Close Team");
  assert.equal(confirmations, 0);
  await act(async () => resolvePreview({ teams: [team], agentTitles: [], blueprints: [] }));
  assert(host.textContent?.includes("Test Cron"));
  await click("Close Team");
  assert.equal(confirmations, 1);
  await click("Open");
  await click("Cancel");
  assert(signal?.aborted);
  await act(async () => resolvePreview({ teams: [team], agentTitles: [], blueprints: [] }));
  assert.equal(host.querySelector('[role="dialog"]'), null);
  await act(async () => root.unmount());
}
void main();
