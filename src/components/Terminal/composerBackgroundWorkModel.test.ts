import assert from "node:assert/strict";
import { test } from "node:test";
import { BACKGROUND_PROCESS_NAME, BACKGROUND_TASK_NAME, backgroundCommandName, backgroundOwnsTool, backgroundWorkElapsed, composerBackgroundWorkBubble, composerBackgroundWorkItems, withProviderBackgroundWork } from "./composerBackgroundWorkModel";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const roster = (running: Record<string, string>, kinds: Record<string, "shell" | "subagent"> = {}, unknown: Record<string, string> = {}) => ({
  running, kinds, unknown, overflow: false, history_incomplete: false, history_initialized: true,
});
const now = Date.parse("2026-10-17T12:00:00.000Z");
test("provider background commands keep their task kind and own-session identity", () => {
  const messages: NormalizedChatMessage[] = [{
    id: "shell-result", type: "tool_call", content: "", sourceSessionId: "own-session",
    toolKind: "command", toolInput: { command: "node fixture.mjs" }, toolId: "call", toolCallSourceId: "source",
  }, {
    id: "shell-outcome", type: "tool_result", content: "", sourceSessionId: "own-session",
    toolId: "call", toolCallSourceId: "source", toolPairing: "exact", toolBackgroundTaskId: "shell",
  }];
  const items = withProviderBackgroundWork([], roster({ shell: "", child: "Inspect" }, { shell: "shell", child: "subagent" }), "own-session", messages);
  assert.deepEqual(items.map(item => [item.kind, item.name]), [["shell", "fixture.mjs"], ["agent", "Inspect"]]);
  assert.equal(items[0].command, "node fixture.mjs");
  assert.equal(items[0].processId, undefined);
  const [noOwnCommand] = withProviderBackgroundWork([], roster({ shell: "" }, { shell: "shell" }), "foreign-session", messages);
  assert.equal(noOwnCommand.kind, "shell");
  assert.equal(noOwnCommand.command, undefined);
  assert.equal(withProviderBackgroundWork([], roster({ shell: "" }), "own-session", messages)[0].kind, "unknown");
});
test("PTY background roster remains visible without runtime agent_activity rows and drains on completion", () => {
  const items = withProviderBackgroundWork([], roster({ child: "Inspect source" }, { child: "subagent" }), "own-session");
  assert.equal(composerBackgroundWorkBubble(items)?.count, 1);
  assert.equal(items[0].status, "running");
  assert.equal(items[0].elapsed, "");
  assert.equal(items[0].transcriptThreadId, undefined);
  assert.equal(withProviderBackgroundWork(items, roster({ child: "Inspect source" }), "own-session").length, 1);
  assert.deepEqual(withProviderBackgroundWork([], roster({}), "own-session"), []);
  assert.deepEqual(withProviderBackgroundWork([], roster({ child: "Foreign" })), []);
});
const agent = (id: string, ageMs: number, toolInput: Record<string, unknown>, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({
  id,
  type: "tool_call",
  content: "",
  timestamp: new Date(now - ageMs).toISOString(),
  sourceSessionId: "root",
  toolKind: "agent",
  toolName: "agent_activity",
  toolInput,
  ...patch,
});

const process = {threadId:"root",processId:"process-1",itemId:"terminal-item",command:"node build.mjs",cwd:"C:/work",osPid:42};

test("agent descriptions survive later activity and take priority over opaque paths", () => {
  const items = composerBackgroundWorkItems([
    agent("start", 30_000, { agent_call_id: "call", agent_state: "started", agent_description: "Inspect source" }),
    agent("running", 20_000, { agent_call_id: "call", agent_id: "a743f86e11053a44a", agent_path: "a743f86e11053a44a", agent_state: "running" }),
  ], [], now, { subagentsVisible: true, rootThreadId: "root" });
  assert.equal(items[0].name, "Inspect source");
});

test("maps running terminals and capable-provider subagents into one counted bubble", () => {
  const items = composerBackgroundWorkItems([
    agent("agent-item", 1, {
      agent_id:"child-thread",agent_call_id:"call-1",agent_state:"running",agent_description:"Inspect the launch path",agent_model:"model-x",agent_reasoning_effort:"high",agent_binding_nonce:"nonce",agent_runtime_epoch:1,
    }),
    {id:"terminal-item",type:"tool_result",content:"build output",timestamp:new Date(now - 2).toISOString()},
  ], [process], now, { subagentsVisible: true, rootThreadId: "root", authority:{rootThreadId:"root",bindingNonce:"nonce",epoch:1} });

  assert.deepEqual(items.map(({kind,threadId}) => [kind,threadId]), [["terminal","root"],["agent","child-thread"]]);
  assert.equal(items[0].outputTail,"build output");
  assert.equal(items[0].elapsed,"0s");
  assert.equal(items[1].prompt,"Inspect the launch path");
  assert.equal(items[1].model,"model-x");
  assert.equal(items[1].effort,"high");
  assert.equal(items[1].elapsed,"0s");
  assert.equal(items[1].status,"running");
  assert.deepEqual(composerBackgroundWorkBubble(items), { count: 2, items });
});

test("historical children without live evidence never raise a bubble", () => {
  const items = composerBackgroundWorkItems([
    agent("a", 1, {agent_id:"a",agent_call_id:"call-a",agent_state:"running"}),
    agent("b", 2, {agent_id:"b",agent_call_id:"call-b",agent_state:"started"}),
    agent("c", 3, {agent_id:"c",agent_call_id:"call-c",agent_state:"running"}),
    agent("foreign", 4, {agent_id:"foreign",agent_call_id:"call-foreign",agent_parent_thread_id:"other-root",agent_state:"running"}),
  ], [], now, { subagentsVisible: true, rootThreadId: "root" });
  const bubble = composerBackgroundWorkBubble(items);

  assert.deepEqual(items.map(({threadId, status}) => [threadId, status]), [["c", "unknown"], ["b", "unknown"], ["a", "unknown"]]);
  assert.equal(bubble, null);
});

test("shows yielded commands immediately, including before their transcript arrives, and clears them on exit", () => {
  const message: NormalizedChatMessage = {id:process.itemId,type:"tool_call",content:"",timestamp:new Date(now).toISOString()};
  const unknownMessage = { ...process, processId:"other", itemId:"not-yet-loaded" };
  const items = composerBackgroundWorkItems([message], [process, unknownMessage], now);

  assert.equal(items[0].elapsed, "0s");
  assert.equal(items[1].elapsed, "");
  assert.equal(composerBackgroundWorkBubble(items)?.count, 2);
  assert.equal(composerBackgroundWorkBubble(composerBackgroundWorkItems([message], [], now + 1)), null);
});

test("filters completed and untimed child work and gates children by provider capability", () => {
  const messages = [
    agent("done-started", 30, {agent_id:"done",agent_state:"running"}),
    agent("done", 20, {agent_id:"done",agent_state:"completed"}),
    { ...agent("untimed", 20, {agent_id:"untimed",agent_state:"running"}), timestamp: undefined },
  ];

  assert.deepEqual(composerBackgroundWorkItems(messages, [], now, { subagentsVisible: true, rootThreadId: "root" }), []);
  assert.deepEqual(composerBackgroundWorkItems([agent("live", 1, {agent_id:"live",agent_state:"running"})], [], now), []);
});

test("formats elapsed time without negative values", () => {
  assert.equal(backgroundWorkElapsed(-1), "0s");
  assert.equal(backgroundWorkElapsed(65_000), "1m");
  assert.equal(backgroundWorkElapsed(3_725_000), "1h 2m");
});

test("restored child activity stays unknown without elapsed time until current binding evidence arrives",()=>{
  const input={agent_id:"child",agent_state:"started",agent_binding_nonce:"old",agent_runtime_epoch:2};
  const options={subagentsVisible:true,rootThreadId:"root",authority:{rootThreadId:"root",bindingNonce:"new",epoch:3}};
  const rows=[agent("child",60_000,input)];
  assert.equal(composerBackgroundWorkItems(rows,[],now,options)[0].status,"unknown");
  assert.equal(composerBackgroundWorkItems(rows,[],now,options)[0].elapsed,"");
  const refreshed=[agent("child",60_000,{...input,agent_binding_nonce:"new",agent_runtime_epoch:3,agent_state:"running"})];
  assert.equal(composerBackgroundWorkItems(refreshed,[],now,options)[0].status,"running");
  assert.equal(composerBackgroundWorkItems(refreshed,[],now,options)[0].elapsed,"1m");
});

test("projects readable command names without long arguments",()=>{
  assert.equal(backgroundCommandName('pwsh -NoProfile -File "C:\\work area\\cleanup-merged.ps1" -Lane example'),"cleanup-merged.ps1");
  assert.equal(backgroundCommandName('bash -lc "node /work/build.mjs --flag value"'),"build.mjs");
  assert.equal(backgroundCommandName(`node ${"x".repeat(40)}.mjs`).length,24);
  assert.equal(backgroundCommandName("bash scripts/build.sh check 2>&1 | tee -a /tmp/build-check.log"),"build.sh check");
  assert.equal(backgroundCommandName('bash -lc "./build.sh fmt && bash scripts/check-file-sizes.sh --fail"'),"build.sh fmt");
  assert.equal(backgroundCommandName("pnpm dev"),"pnpm dev");
  assert.equal(backgroundCommandName("cargo fmt"),"cargo fmt");
  assert.equal(backgroundCommandName("npx vitest run"),"vitest run");
});

test("deduplicates process identity while preserving distinct jobs and exact tool ownership",()=>{
  const message:NormalizedChatMessage={id:process.itemId,type:"tool_call",content:"",timestamp:new Date(now-20_000).toISOString()};
  const items=composerBackgroundWorkItems([message],[process,process,{...process,processId:"another",itemId:"other-item"}],now);
  assert.equal(items.length,2);
  assert.equal(items[0].status,"running");
  assert.equal(backgroundOwnsTool({id:process.itemId,sourceSessionId:"root"},items),true);
  assert.equal(backgroundOwnsTool({id:process.itemId,sourceSessionId:"foreign"},items),false);
  assert.equal(backgroundOwnsTool({id:"foreground"},items),false);
  assert.equal(backgroundOwnsTool(undefined,items),false);
});

test("groups a process tree with subshells, tee, and sccache into one top-level card", () => {
  // 1. Backend-grouped OS entry carrying live children
  const osGrouped = [{
    threadId: "root",
    processId: "os:41208:50",
    itemId: "os-bg:41208:50",
    command: "bash scripts/build.sh check 2>&1 | tee -a /tmp/build-check.log",
    cwd: "/repo",
    osPid: 41208,
    parentOsPid: 41200,
    children: [
      { command: "bash scripts/build.sh check", osPid: 41231 },
      { command: "tee -a /tmp/build-check.log", osPid: 41209 },
      { command: "sccache", osPid: 40977 },
    ],
  }];
  const osItems = composerBackgroundWorkItems([], osGrouped, now);
  assert.equal(osItems.length, 1);
  assert.equal(composerBackgroundWorkBubble(osItems)?.count, 1);
  assert.equal(osItems[0].name, "build.sh check");
  assert.equal(osItems[0].helperCount, 3);
  assert.deepEqual(
    osItems[0].processes?.map(proc => [proc.role, proc.osPid, proc.command]),
    [
      ["main", 41208, "bash scripts/build.sh check 2>&1 | tee -a /tmp/build-check.log"],
      ["helper", 41231, "bash scripts/build.sh check"],
      ["helper", 41209, "tee -a /tmp/build-check.log"],
      ["helper", 40977, "sccache"],
    ],
  );

  // 2. Flat process tree linked by parentOsPid + shared pipeline parent
  const flatTree = [
    { threadId: "root", processId: "p-tee", itemId: "i-tee", command: "tee -a /tmp/build-check.log", cwd: "/repo", osPid: 41209, parentOsPid: 41200 },
    { threadId: "root", processId: "p-main", itemId: "i-main", command: "bash scripts/build.sh check", cwd: "/repo", osPid: 41208, parentOsPid: 41200 },
    { threadId: "root", processId: "p-sub", itemId: "i-sub", command: "bash scripts/build.sh check", cwd: "/repo", osPid: 41231, parentOsPid: 41208 },
    { threadId: "root", processId: "p-sc", itemId: "i-sc", command: "sccache", cwd: "/repo", osPid: 40977, parentOsPid: 41231 },
  ];
  const treeItems = composerBackgroundWorkItems([], flatTree, now);
  assert.equal(treeItems.length, 1);
  assert.equal(composerBackgroundWorkBubble(treeItems)?.count, 1);
  assert.equal(treeItems[0].name, "build.sh check");
  assert.equal(treeItems[0].processId, "p-main");
  assert.equal(treeItems[0].helperCount, 3);
  assert.equal(backgroundOwnsTool({ id: "i-sub", sourceSessionId: "root" }, treeItems), true);
  // Collapsed member processIds must not be re-added from the provider roster.
  const merged = withProviderBackgroundWork(treeItems, roster({ "p-tee": "tee", "p-sc": "sccache" }, { "p-tee": "shell", "p-sc": "shell" }), "root");
  assert.equal(merged.length, 1);

  // 3. Multiple provider processes sharing one launching tool call itemId
  const sameItem = [
    { threadId: "root", processId: "codex-1", itemId: "exec-1", command: "bash scripts/build.sh check", cwd: "/repo", osPid: null },
    { threadId: "root", processId: "codex-2", itemId: "exec-1", command: "tee -a /tmp/build-check.log", cwd: "/repo", osPid: null },
  ];
  const itemGrouped = composerBackgroundWorkItems([], sameItem, now);
  assert.equal(itemGrouped.length, 1);
  assert.equal(itemGrouped[0].name, "build.sh check");
  assert.equal(itemGrouped[0].helperCount, 1);
});

test("never groups unrelated commands by name guesses alone and keeps standalone helpers visible", () => {
  // Two independent user commands spawned directly under the same parent PID stay distinct.
  const twoCommands = [
    { threadId: "root", processId: "p1", itemId: "i1", command: "cargo fmt", cwd: "/repo", osPid: 200, parentOsPid: 100 },
    { threadId: "root", processId: "p2", itemId: "i2", command: "pnpm dev", cwd: "/repo", osPid: 300, parentOsPid: 100 },
  ];
  const twoItems = composerBackgroundWorkItems([], twoCommands, now);
  assert.equal(twoItems.length, 2);
  assert.equal(composerBackgroundWorkBubble(twoItems)?.count, 2);
  assert.deepEqual(twoItems.map(item => item.name), ["cargo fmt", "pnpm dev"]);

  // Unrelated tee/sccache without shared tree or itemId are NOT merged by name alone.
  const unrelated = [
    { threadId: "root", processId: "p1", itemId: "i1", command: "bash scripts/build.sh check", cwd: "/repo", osPid: 200 },
    { threadId: "root", processId: "p2", itemId: "i2", command: "sccache", cwd: "/other", osPid: 300 },
  ];
  assert.equal(composerBackgroundWorkItems([], unrelated, now).length, 2);

  // Standalone surviving helper (root exited) stays visible.
  const surviving = [
    { threadId: "root", processId: "os:40977:52", itemId: "os-bg:40977:52", command: "sccache", cwd: "/repo", osPid: 40977 },
  ];
  const survivingItems = composerBackgroundWorkItems([], surviving, now);
  assert.equal(survivingItems.length, 1);
  assert.equal(survivingItems[0].name, "sccache");
  assert.equal(survivingItems[0].helperCount, 0);
});

test("only live work makes a bubble; unknown work rides along beside it and ended work is never shown", () => {
  const items = withProviderBackgroundWork([], roster({ live: "Live task" }, {}, { dropped: "Dropped task" }), "root");
  assert.deepEqual(items.map(item => item.status), ["running", "unknown"]);
  assert.equal(composerBackgroundWorkBubble(items)?.count, 1);
  assert.deepEqual(composerBackgroundWorkBubble(items)?.items.map(item => item.name), ["Live task", "Dropped task"]);
  assert.equal(composerBackgroundWorkBubble(items.slice(1)), null, "without running work there is no bubble at all");
});

test("unlabelled provider tasks read as numbered sub-agents and keep their raw id aside", () => {
  const items = withProviderBackgroundWork([], roster({ bm76h90j0: "", a2: "Run the release gate", zz9: "  " }, { a2: "subagent", bm76h90j0: "subagent", zz9: "subagent" }), "root");
  assert.deepEqual(items.map(item => [item.name, item.ordinal ?? null, item.rawId]), [
    ["Sub-agent", 1, "bm76h90j0"],
    ["Run the release gate", null, "a2"],
    ["Sub-agent", 2, "zz9"],
  ]);
});

test("own-session compaction invalidates runtime child liveness until fresh activity", () => {
  const authority = {rootThreadId:"root",bindingNonce:"nonce",epoch:1};
  const options = {subagentsVisible:true,rootThreadId:"root",authority};
  const child = agent("start", 60_000, {agent_id:"child",agent_state:"running",agent_binding_nonce:"nonce",agent_runtime_epoch:1});
  const boundary: NormalizedChatMessage = {id:"boundary",type:"system",content:"",sourceSessionId:"root",timelineMarker:{kind:"context_compaction"}};
  const invalidated = composerBackgroundWorkItems([child,boundary],[],now,options);
  assert.equal(invalidated[0].status,"unknown");
  assert.equal(invalidated[0].elapsed,"");
  assert.equal(composerBackgroundWorkBubble(invalidated),null);
  const fresh = composerBackgroundWorkItems([child,boundary,{...child,id:"fresh"}],[],now,options);
  assert.equal(fresh[0].status,"running");
  assert.equal(composerBackgroundWorkBubble(fresh)?.count,1);
  const foreign = composerBackgroundWorkItems([child,{...boundary,sourceSessionId:"foreign"}],[],now,options);
  assert.equal(foreign[0].status,"running");
});

test("provider background shell transitions from running to hidden on completion without counting unknown entries", () => {
  const kinds = { activeA: "shell", activeB: "shell", staleA: "shell", staleB: "shell" } as const;
  const started = withProviderBackgroundWork(
    [],
    roster({ activeA: "", activeB: "" }, kinds, { staleA: "", staleB: "" }),
    "own-session",
  );
  const runningBubble = composerBackgroundWorkBubble(started);
  assert.equal(runningBubble?.count, 2);
  assert.deepEqual(
    runningBubble?.items.map(item => [item.rawId, item.status]),
    [["activeA", "running"], ["activeB", "running"], ["staleA", "unknown"], ["staleB", "unknown"]],
  );

  const oneDone = withProviderBackgroundWork(
    [],
    { ...roster({ activeB: "" }, kinds, { staleA: "", staleB: "" }), ended: { activeA: "" } },
    "own-session",
  );
  const partialBubble = composerBackgroundWorkBubble(oneDone);
  assert.equal(partialBubble?.count, 1);
  assert.equal(partialBubble?.items.some(item => item.rawId === "activeA"), false);

  const allDone = withProviderBackgroundWork(
    [],
    { ...roster({}, kinds, { staleA: "", staleB: "" }), ended: { activeA: "", activeB: "" } },
    "own-session",
  );
  assert.equal(composerBackgroundWorkBubble(allDone), null);
});

test("provider background shells map parallel stream commands, roster commands, and never expose raw task ids", () => {
  // 1. Parallel commands in one shared flow (folded into a stream by reduceToolEvents) both keep their exact commands.
  const parallelMessages: NormalizedChatMessage[] = [
    {
      id: "call-a", type: "tool_call", content: "", sourceSessionId: "own-session",
      toolKind: "command", toolFlowId: "flow-1", toolDescription: "Run first opaque task",
      toolInput: { command: "sleep 900" }, toolId: "tool-a", toolCallSourceId: "asst-a",
    },
    {
      id: "call-b", type: "tool_call", content: "", sourceSessionId: "own-session",
      toolKind: "command", toolFlowId: "flow-1", toolDescription: "Run second opaque task",
      toolInput: { command: "node -e 'setInterval(() => {}, 60000)'" }, toolId: "tool-b", toolCallSourceId: "asst-b",
    },
    {
      id: "res-a", type: "tool_result", content: "", sourceSessionId: "own-session",
      toolId: "tool-a", toolCallSourceId: "asst-a", toolPairing: "exact", toolBackgroundTaskId: "opaque-id-1",
    },
    {
      id: "res-b", type: "tool_result", content: "", sourceSessionId: "own-session",
      toolId: "tool-b", toolCallSourceId: "asst-b", toolPairing: "exact", toolBackgroundTaskId: "opaque-id-2",
    },
  ];
  const fromStream = withProviderBackgroundWork(
    [],
    roster({ "opaque-id-1": "", "opaque-id-2": "" }, { "opaque-id-1": "shell", "opaque-id-2": "shell" }),
    "own-session",
    parallelMessages,
  );
  assert.deepEqual(
    fromStream.map(item => [item.rawId, item.name, item.command]),
    [
      ["opaque-id-1", "sleep 900", "sleep 900"],
      ["opaque-id-2", "node", "node -e 'setInterval(() => {}, 60000)'"],
    ],
  );

  // 2. Unique own-session toolId still maps when sourceToolAssistantUUID points to a sibling row; foreign session never maps.
  const siblingSourceMessages: NormalizedChatMessage[] = [
    {
      id: "call-c", type: "tool_call", content: "", sourceSessionId: "own-session",
      toolKind: "command", toolInput: { command: "opaque-runner --watch" }, toolId: "tool-c", toolCallSourceId: "asst-tool",
    },
    {
      id: "res-c", type: "tool_result", content: "", sourceSessionId: "own-session",
      toolId: "tool-c", toolCallSourceId: "asst-sibling", toolPairing: "exact", toolBackgroundTaskId: "opaque-id-3",
    },
  ];
  const [mappedSibling] = withProviderBackgroundWork(
    [],
    roster({ "opaque-id-3": "" }, { "opaque-id-3": "shell" }),
    "own-session",
    siblingSourceMessages,
  );
  assert.equal(mappedSibling.name, "opaque-runner");
  assert.equal(mappedSibling.command, "opaque-runner --watch");

  const [foreignUnmapped] = withProviderBackgroundWork(
    [],
    roster({ "opaque-id-3": "" }, { "opaque-id-3": "shell" }),
    "foreign-session",
    siblingSourceMessages,
  );
  assert.equal(foreignUnmapped.name, BACKGROUND_PROCESS_NAME);
  assert.equal(foreignUnmapped.command, undefined);
  assert.equal(foreignUnmapped.rawId, "opaque-id-3");

  // 3. Roster command labels from the provider capability populate card name & full command even with no transcript rows.
  const fromRoster = withProviderBackgroundWork(
    [],
    roster(
      { "opaque-id-a": "sleep 900", "opaque-id-b": "node -e 'setInterval(() => {}, 60000)'" },
      { "opaque-id-a": "shell", "opaque-id-b": "shell" },
    ),
    "own-session",
    [],
  );
  assert.deepEqual(
    fromRoster.map(item => [item.name, item.command, item.prompt]),
    [
      ["sleep 900", "sleep 900", undefined],
      ["node", "node -e 'setInterval(() => {}, 60000)'", undefined],
    ],
  );

  // 4. Unknown or unlabelled provider tasks use neutral labels and never render raw task ids.
  const unlabelled = withProviderBackgroundWork(
    [],
    roster({ "raw-shell-99": "", "raw-unknown-88": "" }, { "raw-shell-99": "shell" }),
    "own-session",
    [],
  );
  assert.deepEqual(
    unlabelled.map(item => [item.kind, item.name, item.rawId, item.command]),
    [
      ["shell", BACKGROUND_PROCESS_NAME, "raw-shell-99", undefined],
      ["unknown", BACKGROUND_TASK_NAME, "raw-unknown-88", undefined],
    ],
  );
});
