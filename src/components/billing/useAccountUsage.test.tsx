import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { AccountMeshClientProvider } from "../MeshPanel/accountMeshClient";
import { FakeAccountMeshClient, machine, snapshot } from "../MeshPanel/accountMachines.testdata";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { useAccountUsage } from "./useAccountUsage";

const fixture = vi.hoisted(() => ({ items: 43 as number | undefined, handlers: new Map<string, () => void>() }));
vi.mock("../../lib/webApi", async original => ({ ...await original<typeof import("../../lib/webApi")>(), localFetch: async () => ({ ok: true, json: async () => ({ total_items: fixture.items }) }) }));
vi.mock("../../lib/bridge", async original => ({
  ...await original<typeof import("../../lib/bridge")>(),
  listen: async (event: string, handler: () => void) => { fixture.handlers.set(event, handler); return () => fixture.handlers.delete(event); },
  invoke: vi.fn(),
}));
let root: Root | undefined;
let host: HTMLDivElement;
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
afterEach(async () => { await act(async () => root?.unmount()); host.remove(); fixture.items = 43; fixture.handlers.clear(); });
function Harness() { return <output>{JSON.stringify(useAccountUsage("owner"))}</output>; }
const usage = () => JSON.parse(host.querySelector("output")!.textContent!);
async function render(client: FakeAccountMeshClient, readState?: () => ExtendedState) {
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  const store = readState ? createMuxStateStore(readState) : null;
  await act(async () => root!.render(<MuxStateStoreContext.Provider value={store}>
    <AccountMeshClientProvider value={client}><Harness /></AccountMeshClientProvider>
  </MuxStateStoreContext.Provider>));
  return store;
}

it("reads live local counts, memory stats and active account machines, then refreshes on events", async () => {
  let state = { generation: 1, workspaces: [{ id: "general", protected: true }, { id: "project" }],
    teams: [{ id: "local" }, { id: "remote", remote_peer_name: "peer" }],
    tabs: { shell: { id: "shell" }, mirrored: { id: "mirrored", remote_peer_id: "peer" } } } as unknown as ExtendedState;
  const client = new FakeAccountMeshClient(snapshot({ registry: { observed_at: 1, rows: [machine("a", "Local"), machine("b", "Old", { active: false })] } }));
  const store = await render(client, () => state);
  expect(usage()).toEqual({ workspaces: 1, teams: 1, "agent.tabs": 1, "mesh.machines": 1, "mem.items": 43 });
  fixture.items = 47;
  await act(async () => { fixture.handlers.get("mem-activity")!(); });
  expect(usage()["mem.items"]).toBe(47);
  state = { ...state, tabs: {} };
  await act(async () => store!.notify());
  expect(usage()["agent.tabs"]).toBe(0);
  await act(async () => client.emit(snapshot({ registry: null })));
  expect(usage()["mesh.machines"]).toBeUndefined();
});

it("omits unknown usage and incomplete registry counts", async () => {
  fixture.items = undefined;
  const client = new FakeAccountMeshClient(snapshot({ registry: { observed_at: 1, rows: [machine("a", "Local")], cursor: { after_installation_id: "a", registry_revision: 1 } } }));
  await render(client);
  expect(usage()).toEqual({});
});
