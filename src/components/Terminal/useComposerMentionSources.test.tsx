import { harnessWindow } from "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
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
globalThis.fetch = async (input) => { fetched.push(String(input)); return Response.json(String(input).includes("/fs/list-dir") ? { entries: [{ name: "cdp.mjs", path: "/repo/cdp.mjs" }] } : String(input).includes("chat=1") ? { plans: [{ ...FIXTURE_PLAN, project_path: "/repo", team_id: "team-a", title: `Update ${renders}` }] } : { plans: [] }); };
configureDaemonTransport({ origin: harnessWindow.location.origin, token: "fixture" });
const api = { enabled: true, host: "127.0.0.1", port: 7686, token: "fixture", cors: true };
function Harness() { renders++; const feed = useSidebarPlansOptional(); currentPlanTitle = feed.plans[0]?.title; refetchPlans = feed.refetch; sources = useComposerMentionSources("t"); return null; }
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
  } finally { await act(async () => root.unmount()); await harnessWindow.happyDOM.abort(); }
}
void main().catch((error) => { console.error(error); process.exitCode = 1; });
