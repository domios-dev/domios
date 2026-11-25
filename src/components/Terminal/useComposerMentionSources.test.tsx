import { muxMockFetch } from "../../lib/muxMockFetch.testdata";
import { harnessWindow } from "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MuxStateStoreContext, PairedWorkspaceInventoryContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { useComposerMentionSources } from "./useComposerMentionSources";
import { configureOrchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../../lib/transport/orchestration.testdata";
import { PlansProvider, useSidebarPlansOptional } from "../Canvas/PlansProvider";
import { FIXTURE_PLAN } from "../Canvas/cards/fixtures.testdata";
import { configureDaemonTransport } from "../../lib/daemonBootstrap";
import { __setListenForTest } from "../../lib/bridge";
import { fixtureGraph } from "../Automation/fixtures.testdata";
import { projectAttachedRemoteTab } from "../../lib/remoteTabPortal";
let state = { tabs: { t: { id: "t", title: "Rivet", team_id: "team-a", workspace_id: "ws", tab_type: "Terminal" } }, workspaces: [{ id: "ws", cwd: "/repo", name: "Repo", tabs: ["t"] }] } as unknown as ExtendedState;
const mux = createMuxStateStore(() => state);
const paths: string[] = [];
configureOrchestrationTransport(orchestrationTestClient(async <T,>(_method: string, path: string) => { paths.push(path); return { ...fixtureGraph(), nodes: [] } as T; }));
let sources!: ReturnType<typeof useComposerMentionSources>;
let renders = 0;
let currentPlanTitle: string | undefined;
let refetchPlans!: () => Promise<void>;
const planListeners = new Set<() => void>();
const planChanged = () => { for (const changed of planListeners) changed(); };
const fetched: string[] = [];
__setListenForTest(async (event, handler) => {
  const changed = () => handler({ payload: undefined as never });
  if (event === "plan-changed") planListeners.add(changed);
  return () => { planListeners.delete(changed); };
});
globalThis.fetch = muxMockFetch(async (input) => { fetched.push(String(input)); return Response.json(String(input).includes("/fs/list-dir") ? { entries: [{ name: "cdp.mjs", path: "/repo/cdp.mjs" }] } : (String(input).includes("chat=1") || String(input).includes("view=summary")) ? { plans: [{ ...FIXTURE_PLAN, revision: String(renders + 1), project_path: "/repo", team_id: "team-a", title: `Update ${renders}` }] } : { plans: [] }); });
configureDaemonTransport({ origin: harnessWindow.location.origin, token: "fixture" });
const api = {mesh_tailnet_host: null, cors_allow_origins: [], preview_secret_paths: [],  enabled: true, host: "127.0.0.1", port: 7686, token: "fixture", cors: true };
function Harness({ tabId = "t" }: { tabId?: string }) { renders++; const feed = useSidebarPlansOptional(); currentPlanTitle = feed.plans[0]?.title; refetchPlans = feed.refetch; sources = useComposerMentionSources(tabId); return null; }
async function main() {
  const element = document.createElement("div"); const root = createRoot(element);
  try {
    await act(async () => root.render(<MuxStateStoreContext.Provider value={mux}><PlansProvider api={api}><Harness /></PlansProvider></MuxStateStoreContext.Provider>));
    const ctx = () => ({ scope: sources.scope(), signal: new AbortController().signal });
    const groups = new Set((await sources.registry.search("", ctx())).map(row => row.group));
    assert(groups.has("Tasks") && groups.has("Plans") && groups.has("Files"), "mounted composer queries every non-tab source");
    await sources.registry.search("", ctx()); await sources.registry.search("r", ctx()); await sources.registry.search("ri", ctx());
    assert.equal(paths.length, 2, "one scoped Team and workspace read per open picker");
    assert.ok(paths.every((path) => path.startsWith("/api/automation/graph?") && !path.includes("__all_jobs__")));
    const registry = sources.registry;
    let planNotifications = 0;
    const offPlans = registry.subscribe!(() => planNotifications++);
    const previousTitle = currentPlanTitle;
    await act(async () => { await refetchPlans(); planChanged(); });
    assert.notEqual(currentPlanTitle, previousTitle, "the plan feed actually changes");
    assert.equal(planNotifications, 1, "plan updates refresh picker results");
    offPlans();
    assert(fetched.every(path => !path.includes("full=1")), "chat never downloads full plans");
    assert(fetched.some(path => path.includes("query=") && path.includes("team=team-a") && path.includes("project=%2Frepo")), "search is bounded and scoped");
    assert.equal(sources.registry, registry, "plan feed updates preserve the active registry");
    await sources.registry.search("", ctx());
    assert.equal(paths.length, 2, "plan updates reuse scoped graph reads");
    const beforeActivity = renders;
    state = { ...state, tabs: { t: { ...state.tabs.t, agent_state: { state: "working" } as never } } };
    await act(async () => mux.notify());
    assert.equal(renders, beforeActivity, "source hook never subscribes to background mux updates");
    assert.equal(paths.length, 2);
    sources.registry.close?.();
    await sources.registry.search("", ctx()); assert.equal(paths.length, 4, "new open session reads fresh scoped graphs");
    console.log("4 lazy source and scoped Blueprint session-cache checks passed");

    const viewerVela = { ...state.tabs.t, id: "vela-id", title: "Vela", team_id: "viewer-team" };
    const hostWorkspace = { ...state.workspaces[0], id: "host-ws", name: "Host project", cwd: "C:/host", tabs: ["elgar", "vela-id"] };
    const hostElgar = { ...state.tabs.t, id: "elgar", title: "Elgar", team_id: "dev-team", workspace_id: hostWorkspace.id };
    const hostVela = { ...viewerVela, workspace_id: hostWorkspace.id, team_id: "design-team" };
    const attachment = projectAttachedRemoteTab(hostElgar, { peer_id: "blackstar", peer_name: "BLACKSTAR" }, "ws");
    state = { ...state, tabs: { ...state.tabs, [viewerVela.id]: viewerVela, [attachment.id]: attachment } };
    const showRemoteComposer = async (availability: "online" | "offline", velaTeam = "design-team") => {
      const peerWorkspaces = [{ peerId: "blackstar", workspace: hostWorkspace, teams: [],
        tabs: [{ viewerId: attachment.id, tab: hostElgar }, { viewerId: "mesh:blackstar/vela-id", tab: { ...hostVela, team_id: velaTeam } }],
      }];
      await act(async () => root.render(<MuxStateStoreContext.Provider value={mux}>
        <PairedWorkspaceInventoryContext.Provider value={{
          generals: [{ peerId: "blackstar", name: "BLACKSTAR", sessions: [], availability }], browse: null, peerWorkspaces,
        }}>
          <PlansProvider api={api}><Harness tabId={attachment.id} /></PlansProvider>
        </PairedWorkspaceInventoryContext.Provider>
      </MuxStateStoreContext.Provider>));
    };
    await showRemoteComposer("online");
    assert.equal(sources.scope().peerId, "blackstar");
    assert.equal(sources.scope().workspaceId, "host-ws");
    assert.equal(sources.scope().teamId, "dev-team");
    for (const query of ["vel", "Vela"]) {
      const [vela, ...other] = await sources.registry.search(query, ctx());
      assert.equal(vela.insertText, '<domios type="tab" id="mesh:blackstar/vela-id" />');
      assert.equal(vela.group, "Tabs · Other Teams");
      assert.equal(other.length, 0);
    }
    await showRemoteComposer("online", "dev-team");
    assert.equal((await sources.registry.search("Vela", ctx()))[0].group, "Tabs · Team");
    await showRemoteComposer("offline");
    assert.deepEqual(await sources.registry.search("Vela", ctx()), []);
    console.log("Remote composer follows authenticated host inventory, including unattached tabs and Team moves.");
  } finally { await act(async () => root.unmount()); await harnessWindow.happyDOM.abort(); }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
