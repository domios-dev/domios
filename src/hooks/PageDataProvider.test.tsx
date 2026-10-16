import assert from "node:assert/strict";
import { act, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { PageDataProvider } from "./PageDataProvider";
import { defaultConfig, useConfig } from "./useConfig";
import { useShellProfiles } from "./useShellProfiles";
import { useCronJobs } from "../components/Jobs/useCronJobs";
import { ALL_JOBS_SCOPE } from "../components/Automation/types";
import { useWorkspaceCrons } from "./useWorkspaceCrons";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { configureOrchestrationTransport } from "../lib/transport/orchestrationRuntime";
import { orchestrationTestClient } from "../lib/transport/orchestration.testdata";
import { hostAutomationAdapter, AUTOMATION_GRAPH_EVENT } from "../components/Automation/hostAdapter";
import { attachment, fixtureGraph } from "../components/Automation/fixtures.testdata";

async function main() {
const scope = { ...attachment, connection_id: "sidebar", team_id: ALL_JOBS_SCOPE };
const win = new Window({ url: "http://127.0.0.1:7686" });
Object.assign(win, { __CODETERM_DAEMON__: { origin: "http://127.0.0.1:7686", token: "test-token" } });
for (const key of ["window", "document", "navigator", "HTMLElement", "Node"]) {
  Object.defineProperty(globalThis, key, { value: win[key as keyof typeof win], configurable: true });
}
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const calls = new Map<string, number>();
const count = (name: string) => calls.set(name, (calls.get(name) ?? 0) + 1);
const events = new Map<string, Set<(event: { payload: unknown }) => void>>();
__setListenForTest(async (name, handler) => {
  const handlers = events.get(name) ?? new Set();
  handlers.add(handler as (event: { payload: unknown }) => void);
  events.set(name, handlers);
  return () => { handlers.delete(handler as (event: { payload: unknown }) => void); };
});
__setInvokeForTest(async <T,>(command: string) => {
  count(command);
  return (command === "get_config" ? defaultConfig() : []) as T;
});
const graphEvents = new Set<(name: string, payload: unknown) => void>();
configureOrchestrationTransport(orchestrationTestClient(async <T,>() => {
  count("graph"); return fixtureGraph() as T;
}, (_stream, handler) => {
  graphEvents.add(handler);
  return () => { graphEvents.delete(handler); };
}));
let observedTheme = "";
function WorkspaceConsumer() {
  const { config } = useConfig();
  useShellProfiles();
  useWorkspaceCrons("workspace-test");
  useCronJobs("workspace-test");
  observedTheme = config.theme.name;
  useEffect(() => {
    const controller = new AbortController();
    void hostAutomationAdapter.read(scope, controller.signal);
    let off: (() => void) | undefined;
    void hostAutomationAdapter.subscribe(scope, () => { void hostAutomationAdapter.read(scope, controller.signal); }).then((stop) => { off = stop; });
    return () => { off?.(); };
  }, []);
  return null;
}
const root = createRoot(document.createElement("div"));
async function render(open: boolean) {
  await act(async () => { root.render(<PageDataProvider>{open && <><WorkspaceConsumer /><WorkspaceConsumer /></>}</PageDataProvider>); });
}
await render(true);
assert.deepEqual(Object.fromEntries(calls), { get_config: 1, shell_profiles: 1, cron_list: 1, graph: 1 });
await render(true);
await render(false);
await render(true);
assert.deepEqual(Object.fromEntries(calls), { get_config: 1, shell_profiles: 1, cron_list: 1, graph: 1 });
console.log("✓ four page loads stay shared across consumers and drawer remounts");
await act(async () => {
  const config = defaultConfig();
  config.theme = { ...config.theme, name: "opaque-theme" };
  for (const handler of events.get("config-changed") ?? []) handler({ payload: config });
  for (const handler of events.get("shells-changed") ?? []) handler({ payload: null });
  for (const handler of events.get("cron-run-finished") ?? []) handler({ payload: null });
  for (const handler of graphEvents) handler(AUTOMATION_GRAPH_EVENT, { ...attachment, revision: "revision-2" });
});
assert.equal(observedTheme, "opaque-theme");
assert.deepEqual(Object.fromEntries(calls), { get_config: 1, shell_profiles: 2, cron_list: 2, graph: 2 });
console.log("✓ each change invalidates once; config uses its payload without a GET");
await render(false);
await act(async () => {
  for (const handler of graphEvents) handler(AUTOMATION_GRAPH_EVENT, { ...attachment, revision: "revision-3" });
});
assert.equal(calls.get("graph"), 3);
await render(true);
assert.deepEqual(Object.fromEntries(calls), { get_config: 1, shell_profiles: 2, cron_list: 2, graph: 3 });
console.log("✓ cached scopes refresh while the drawer is closed without adding reads on reopening");
await act(async () => { root.unmount(); });
__setInvokeForTest(null);
__setListenForTest(null);
await win.happyDOM.close();

}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
