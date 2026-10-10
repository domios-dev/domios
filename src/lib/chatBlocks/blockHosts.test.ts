// Run: npx tsx src/lib/chatBlocks/blockHosts.test.ts

import { Window } from "happy-dom";
import { FIXTURE_TASK } from "../../components/Canvas/cards/fixtures.testdata";
import type { Plan, Task } from "../../components/Canvas/types";
import type { AutomationGraph, BlueprintSnapshot } from "../../components/Automation/types";
import type { PluginSnapshotStoreState } from "../pluginSnapshotStore";
import type { CronJob, MultiplexerState, Tab } from "../types";
import { agentBlockInfo, agentRunState, resolveTabRef } from "./agentBlockState";
import { createAgentBlockHost } from "./agentBlockHost";
import { choiceRepliesByMessage, createChoiceBlockHost } from "./choiceBlockHost";
import { agentSpawned, blueprintCreated, checklistInfo, GETTING_STARTED, memorySaved, phoneConnected, pluginInstalled } from "./checklists";
import { createChecklistBlockHost } from "./checklistBlockHost";
import { createJobBlockHost } from "./jobBlockHost";
import { jobBlockState } from "./jobBlockState";
import { createJobsBlockStore, type JobsSnapshot } from "./jobsBlockStore";
import { tasksBlockState } from "./tasksBlockHost";
import { createValueFeed } from "./valueFeed";

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: testWindow, configurable: true });

const tests: Array<[string, () => void | Promise<void>]> = [];
function test(name: string, run: () => void | Promise<void>) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function tab(id: string, extra: Partial<Tab> = {}): Tab {
  return { id, title: `Tab ${id}`, pty_id: `pty-${id}`, tab_type: "Terminal", cwd: null, agent_type: "claude", ...extra } as Tab;
}

function task(id: string, key: string, status: Task["status"], extra: Partial<Task> = {}): Task {
  return { ...FIXTURE_TASK, id, key, name: `Task ${key}`, status, milestones: [], ...extra };
}

function plan(id: string, tasks: Task[]): Plan {
  return { id, title: `Plan ${id}`, status: "active", tasks, team_id: null, updated_at: 1 } as unknown as Plan;
}

function cron(id: string, extra: Partial<CronJob> = {}): CronJob {
  return {
    id, name: `Cron ${id}`, description: null, workspace_id: "ws", schedule: "0 9 * * *", enabled: true, agent: "claude", model: null,
    reasoning_effort: null, task: "", cwd: null, role: null, oneshot: false, max_history: 5, created_at: 1, last_run: null, next_run: null, jitter_max_ms: null,
    ...extra,
  };
}

function graph(blueprints: Partial<BlueprintSnapshot>[]): AutomationGraph {
  return {
    workspace_id: "ws", team_id: null, scenarios: [], revision: "1", nodes: [], edges: [],
    blueprints: blueprints.map((entry) => ({ id: "bp", name: "Blueprint", state: "idle", run_id: null, current_step: null, last_event_at_ms: null, ...entry })),
  } as unknown as AutomationGraph;
}

const TABS: Record<string, Tab> = {
  "e3ec9ee9-1111": tab("e3ec9ee9-1111", { title: "Fermi", role: "Worker", role_profile: "worker", agent_state: { state: "working" }, agent_session_model: "opus" }),
  "e3ec0000-2222": tab("e3ec0000-2222", { title: "Ada", agent_state: { state: "question" } }),
  "general-1": tab("general-1", { title: "General", role: "Manager", role_profile: "general_agent" }),
};

test("agent_ref_resolves_exact_or_unique_prefix_only", () => {
  assert(resolveTabRef(TABS, "e3ec9ee9-1111")?.title === "Fermi", "exact id");
  assert(resolveTabRef(TABS, "e3ec9ee9")?.title === "Fermi", "unique prefix");
  assert(resolveTabRef(TABS, "e3ec") === null, "ambiguous prefix must not guess");
});

test("agent_state_reads_the_live_classification", () => {
  assert(agentRunState(TABS["e3ec9ee9-1111"]) === "working", "working");
  assert(agentRunState(TABS["e3ec0000-2222"]) === "waiting", "question is waiting");
  assert(agentRunState(tab("x", { agent_booting: true })) === "starting", "booting");
  assert(agentRunState(tab("x", { pty_id: null })) === "offline", "no runtime is offline");
  const busy = plan("pl_a", [task("task_1", "AL-1", "in_progress", { assigned_tab_id: "e3ec9ee9-1111" })]);
  const info = agentBlockInfo(TABS["e3ec9ee9-1111"], [busy], () => "Claude");
  assert(info.task?.key === "AL-1" && info.role === "Worker" && info.model === "opus" && info.provider === "Claude" && info.canStop, `info ${JSON.stringify(info)}`);
  assert(!agentBlockInfo(TABS["e3ec0000-2222"], [], () => null).canStop, "only a working agent can be stopped");
});

test("agent_host_reads_stably_opens_and_stops_through_commands", async () => {
  const state: Pick<MultiplexerState, "tabs" | "workspaces"> = { tabs: TABS, workspaces: [{ id: "ws", tabs: Object.keys(TABS) } as never] };
  const calls: string[] = [];
  const host = createAgentBlockHost({
    mux: { getState: () => state, subscribe: () => () => {} },
    plans: createValueFeed<readonly Plan[]>([]),
    providerName: () => "Claude",
    selectTab: (workspaceId, tabId) => { calls.push(`select:${workspaceId}/${tabId}`); },
    interrupt: async (args) => { calls.push(`interrupt:${args.ptyId}`); },
  });
  assert(host.binding.read("e3ec9ee9") === host.binding.read("e3ec9ee9"), "read must be stable between changes");
  assert(calls.length === 0, "reading acted");
  host.open("e3ec9ee9");
  await host.stop("e3ec9ee9");
  assert(calls.join(",") === "select:ws/e3ec9ee9-1111,interrupt:pty-e3ec9ee9-1111", calls.join(","));
  assert(host.binding.read("nope-nope").kind === "missing", "unknown tab is missing");
});

test("agent_host_invalidates_activity_when_only_tab_status_changes", () => {
  let tabStatuses = { "e3ec9ee9-1111": "Idle" as const } as Record<string, import("../types").TabStatus>;
  const host = createAgentBlockHost({
    mux: { getState: () => ({ tabs: TABS, workspaces: [], tabStatuses }), subscribe: () => () => {} },
    plans: createValueFeed<readonly Plan[]>([]),
    providerName: () => "Fixture",
    selectTab: () => {},
    interrupt: async () => {},
  });
  for (const [status, expected, canStop] of [
    ["Idle", "idle", false], ["Working", "working", true], ["Waiting", "waiting", false],
  ] as const) {
    tabStatuses = { "e3ec9ee9-1111": status };
    const result = host.binding.read("e3ec9ee9");
    assert(result.kind === "ready", "the same tab stays available");
    assert(result.agent.state === expected && result.agent.canStop === canStop, "status-only events update activity and Stop");
  }
});

const JOBS: JobsSnapshot = {
  crons: [cron("cron_a", { name: "Digest", next_run: Date.now() + 3_600_000 }), cron("cron_b", { name: "Twin" }), cron("cron_c", { name: "Twin", enabled: false })],
  cronsError: null,
  graphs: [{ workspaceId: "ws", graph: graph([{ id: "bp_1", name: "Inbox triage", state: "running", trigger: { type: "schedule", summary: "Every 15 minutes" } }]) }],
  graphsError: null,
};

test("job_state_resolves_by_id_or_unique_name_and_stays_honest", () => {
  const digest = jobBlockState(JOBS, "Digest");
  assert(digest.kind === "ready" && digest.job.id === "cron_a" && digest.job.nextRun !== null && digest.job.canPause, "name resolves a cron");
  assert(jobBlockState(JOBS, "Twin").kind === "missing", "ambiguous name must not guess");
  const paused = jobBlockState(JOBS, "cron_c");
  assert(paused.kind === "ready" && paused.job.state === "paused" && paused.job.nextRun === null, "disabled cron is paused");
  const blueprint = jobBlockState(JOBS, "bp_1");
  assert(blueprint.kind === "ready" && blueprint.job.state === "running" && !blueprint.job.canPause && !blueprint.job.canRunNow, "blueprint has no fake controls");
  assert(blueprint.kind === "ready" && blueprint.job.schedule === "Every 15 minutes", "blueprint trigger is its schedule");
  assert(jobBlockState({ ...JOBS, graphs: null }, "zzz").kind === "loading", "unknown while loading");
  assert(jobBlockState({ ...JOBS, cronsError: "offline" }, "zzz").kind === "unavailable", "failed read is unavailable, not missing");
  const ran = cron("cron_r", { last_run: { run_id: "r", started_at: 1, finished_at: 2, status: "Failed", tab_id: null } });
  const failedRun = jobBlockState({ ...JOBS, crons: [ran] }, "cron_r");
  assert(failedRun.kind === "ready" && failedRun.job.lastResult?.label.startsWith("Failed"), "last result missing");
});

test("jobs_store_reads_only_while_watched_and_coalesces_refreshes", async () => {
  let reads = 0;
  let watches = 0;
  let unwatched = 0;
  const store = createJobsBlockStore({
    listCrons: async () => { reads++; return []; },
    readGraph: async () => graph([]),
    workspaceIds: () => ["ws"],
    watch: () => { watches++; return () => { unwatched++; }; },
  });
  const offA = store.subscribe(() => {});
  const offB = store.subscribe(() => {});
  assert(watches === 1, "watch must attach once");
  await Promise.all([store.refresh(), store.refresh(), store.refresh()]);
  assert(reads === 2 && store.get().crons?.length === 0, `overlapping refreshes read ${reads} times`);
  offA();
  offB();
  assert(unwatched === 1, "watch must detach with the last listener");
});

test("job_host_acts_through_commands_and_refreshes", async () => {
  const calls: string[] = [];
  let refreshes = 0;
  const store = { get: () => JOBS, subscribe: () => () => {}, refresh: async () => { refreshes++; } };
  const host = createJobBlockHost(store, {
    triggerCron: async (id) => { calls.push(`trigger:${id}`); },
    toggleCron: async (id, enabled) => { calls.push(`toggle:${id}:${enabled}`); },
    cancelCron: async (id) => { calls.push(`cancel:${id}`); },
    stopBlueprint: async (_attachment, identity) => { calls.push(`stop:${identity.id}`); },
    openJob: (detail) => { calls.push(`open:${detail.target.cronId ?? detail.target.blueprintId}`); },
  });
  assert(host.binding.read("cron_a") === host.binding.read("cron_a") && calls.length === 0, "reads must be stable and inert");
  await host.runNow("cron_a");
  await host.setPaused("cron_a", true);
  await host.stop("bp_1");
  host.open("bp_1");
  let refused = false;
  await host.setPaused("bp_1", true).catch(() => { refused = true; });
  assert(refused, "blueprint pause must be refused");
  assert(calls.join(",") === "trigger:cron_a,toggle:cron_a:false,stop:bp_1,open:bp_1", calls.join(","));
  assert(refreshes === 3, `each action refreshes once, got ${refreshes}`);
});

test("checklist_signals_tick_from_real_state", () => {
  assert(agentSpawned({ g: TABS["general-1"] }) === false, "the General Agent alone is not a spawned agent");
  assert(agentSpawned(TABS) === true, "a worker tab counts");
  assert(agentSpawned({ h: tab("h", { hidden: true }) }) === false, "hidden workers do not count");
  assert(phoneConnected(null) === null && phoneConnected([{ id: "telegram", label: "Telegram", control_available: true }]) === true, "phone");
  assert(pluginInstalled(false, []) === null && pluginInstalled(true, [{ installed: true, system: true, dev: false } as never]) === false, "bundled plugins do not count");
  assert(pluginInstalled(true, [{ installed: true, system: false, dev: false } as never]) === true, "user plugin counts");
  assert(blueprintCreated({ ...JOBS, graphs: null }) === null && blueprintCreated(JOBS) === true, "blueprint");
  assert(memorySaved(null) === null && memorySaved({ skill: 4 }) === false && memorySaved({ fact: 1 }) === true, "memory ignores seeded skills");
  const info = checklistInfo(GETTING_STARTED, { agentSpawned: true, phoneConnected: false, pluginInstalled: null, blueprintCreated: false, memorySaved: true });
  assert(info.items.map((item) => item.done).join(",") === "true,false,,false,true", `done ${info.items.map((item) => item.done).join(",")}`);
  assert(GETTING_STARTED.items.every((item) => item.action.label.length > 0), "every item has a one-click action");
});

test("checklist_host_follows_sources_and_opens_links", async () => {
  const opened: string[] = [];
  const mux = createValueFeed<Pick<MultiplexerState, "tabs">>({ tabs: {} });
  const plugins = { ...createValueFeed<PluginSnapshotStoreState>({ snapshot: { plugins: [], channels: [], diff: [] }, lastSuccessfulSnapshot: null, error: null, loading: false, refreshing: false, hasLoaded: true, lastLoadedAt: 1 }), ensure: async () => {} };
  const jobs = createJobsBlockStore({ listCrons: async () => [], readGraph: async () => graph([]), workspaceIds: () => [], watch: () => () => {} });
  const host = createChecklistBlockHost({
    mux, plugins, jobs,
    readChannels: async () => [{ id: "telegram", label: "Telegram", control_available: true }],
    readMemoryKinds: async () => ({ fact: 2 }),
    links: { "new-agent": () => opened.push("new-agent"), plugins: () => opened.push("plugins"), jobs: () => opened.push("jobs"), memory: () => opened.push("memory") },
  });
  let notified = 0;
  const off = host.binding.subscribe(() => notified++);
  const first = host.binding.read("getting-started");
  assert(first.kind === "ready" && first.checklist.items[0].done === false, "no agent yet");
  mux.set({ tabs: TABS });
  const second = host.binding.read("getting-started");
  assert(notified > 0 && second.kind === "ready" && second.checklist.items[0].done === true, "agent step did not tick live");
  host.binding.refresh("getting-started");
  await jobs.refresh();
  await Promise.resolve();
  const third = host.binding.read("getting-started");
  assert(third.kind === "ready" && third.checklist.items.find((item) => item.id === "phone")?.done === true && third.checklist.items.find((item) => item.id === "memory")?.done === true, "refresh did not read phone and memory");
  host.open("getting-started", "memory");
  host.open("getting-started", "phone");
  assert(opened.join(",") === "memory", "a block action must not open a link");
  assert(host.binding.read("nope").kind === "missing", "unknown checklist");
  off();
});

test("choice_replies_and_single_claim", async () => {
  const replies = choiceRepliesByMessage([
    { id: "a1", type: "assistant", content: "Ship?" },
    { id: "t1", type: "tool_call", content: "" },
    { id: "a2", type: "assistant", content: "@ds-choice Yes | No" },
    { id: "r1", type: "user", content: "report", inboundKind: "report" },
    { id: "u1", type: "user", content: " Yes " },
    { id: "a3", type: "assistant", content: "Later question" },
  ]);
  assert(replies.get("a1") === "Yes" && replies.get("a2") === "Yes" && !replies.has("a3"), "replies wrong");
  const feed = createValueFeed<ReadonlyMap<string, string>>(new Map());
  const sent: string[] = [];
  const host = createChoiceBlockHost(feed, async (label) => { sent.push(label); });
  const [first, second] = await Promise.all([host.choose("a3", "Yes"), host.choose("a3", "No")]);
  assert(first && !second && sent.join(",") === "Yes", "a message must send once");
  feed.set(new Map([["a4", "No"]]));
  assert(!(await host.choose("a4", "Yes")) && host.binding.read("a4")?.reply === "No", "an answered message must not send");
  const failing = createChoiceBlockHost(createValueFeed<ReadonlyMap<string, string>>(new Map()), async () => { throw new Error("down"); });
  let threw = false;
  await failing.choose("a5", "Yes").catch(() => { threw = true; });
  assert(threw, "send failure must surface");
});

test("tasks_state_keeps_the_block_set_with_live_rows", () => {
  const live = plan("pl_a", [task("task_1", "AL-1", "in_progress"), task("task_2", "AL-2", "done"), task("task_3", "AL-3", "cancelled")]);
  const snapshot = { plans: [live], loaded: true };
  const all = tasksBlockState(snapshot, { planId: "pl_a", refs: [], groups: [], items: null });
  assert(all.kind === "ready" && all.total === 2 && all.title === "Plan pl_a", "default excludes cancelled");
  const picked = tasksBlockState(snapshot, { planId: "pl_a", refs: ["AL-2", "AL-9"], groups: [], items: null });
  assert(picked.kind === "ready" && picked.total === 2 && picked.groups.flatMap((group) => group.rows).some((row) => row.detached && row.key === "AL-9"), "missing ref must stay as a detached row");
  const carried = tasksBlockState(snapshot, { planId: "pl_a", refs: [], groups: [], items: [{ id: "task_1", key: "AL-1", status: "pending", name: "old" }] });
  assert(carried.kind === "ready" && carried.groups[0].rows[0].status === "in_progress", "carried rows must read live status");
  assert(tasksBlockState({ plans: [], loaded: false }, { planId: "pl_a", refs: [], groups: [], items: null }).kind === "loading", "loading");
  assert(tasksBlockState({ plans: [], loaded: true }, { planId: "pl_a", refs: [], groups: [], items: null }).kind === "missing", "missing");
});

(async () => {
  let failed = 0;
  for (const [name, run] of tests) {
    try {
      await run();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  console.log(`chatBlocks/blockHosts tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
  process.exit(failed === 0 ? 0 : 1);
})();
