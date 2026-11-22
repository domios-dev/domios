import assert from "node:assert/strict";
import type { JsonValue, Operation, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";
import { configOptions, sessionSettings } from "../src/runtimeControls";
import { reduceStream, sanitizeSessionTitle, settleStream } from "../src/runtimeReduce";
import { runtimePlan, runtimeReduce, runtimeResponse, selectHostMcpServers, supportsHttpMcp } from "../src/runtime";

const baseCtx: RuntimeContext = {
  epoch: 1,
  cwd: "/work/project",
  hostVersion: "1.21.1",
  tabId: "acp-ux-polish",
  bindingNonce: "nonce-ux",
  settings: {},
  providerSettings: {},
  claimedSends: [],
  activeTurns: [],
  threadParents: [],
};

// 1. session_info_update.title sanitization, deduplication, and compaction coexistence.
{
  assert.equal(sanitizeSessionTitle("  Fix\u0007 login\nredirect\tbug  "), "Fix login redirect bug");
  assert.equal(sanitizeSessionTitle("   \u0000\u001f  "), undefined);
  assert.equal(sanitizeSessionTitle("a".repeat(100))?.length, 64);

  const ctx = { ...baseCtx, threadId: "session-title" };
  const titleFrame = (title: string, compaction?: Record<string, JsonValue>): RuntimeFrame => ({
    kind: "notification",
    method: "session/update",
    params: {
      sessionId: "session-title",
      update: {
        sessionUpdate: "session_info_update",
        title,
        ...(compaction ? { _meta: { compaction } } : {}),
      },
    },
  });
  const first = reduceStream(ctx, null, [titleFrame("  Fix\tauth\u0007 flow  ", { id: "cmp-1" })], { prompt: 1, replay: 0, loading: false });
  assert.deepEqual(
    first.events.filter(event => event.kind === "sessionTitle" || event.kind === "contextCompacted"),
    [
      { kind: "sessionTitle", threadId: "session-title", title: "Fix auth flow" },
      { kind: "contextCompacted", threadId: "session-title", itemId: "cmp-1" },
    ],
  );
  const dup = reduceStream(ctx, first.state, [titleFrame("Fix auth flow")], { prompt: 1, replay: 0, loading: false });
  assert.equal(dup.events.some(event => event.kind === "sessionTitle"), false, "identical title does not re-emit");
  const updated = reduceStream(ctx, dup.state, [titleFrame("Auth flow verified")], { prompt: 1, replay: 0, loading: false });
  assert.deepEqual(
    updated.events.filter(event => event.kind === "sessionTitle"),
    [{ kind: "sessionTitle", threadId: "session-title", title: "Auth flow verified" }],
  );
}

// 2. configOptions: boolean toggles, grouped selects, and descriptions.
{
  const parsed = configOptions([
    {
      id: "web_search",
      name: "Web search",
      description: "Allow live web queries",
      type: "boolean",
      currentValue: true,
    },
    {
      id: "sandbox_profile",
      name: "Sandbox",
      description: "Execution boundary",
      type: "select",
      currentValue: "workspace",
      options: [
        {
          group: "local",
          name: "Local",
          options: [
            { value: "workspace", name: "Workspace only", description: "Confine writes to repo" },
            { value: "read_only", name: "Read only", description: "No file writes" },
          ],
        },
        {
          group: "elevated",
          name: "Elevated",
          options: [
            { value: "full", name: "Full access", description: "Unrestricted tools" },
          ],
        },
      ],
    },
  ]);
  assert.deepEqual(parsed, [
    {
      id: "web_search",
      name: "Web search",
      kind: "boolean",
      description: "Allow live web queries",
      currentValue: "true",
      options: [],
    },
    {
      id: "sandbox_profile",
      name: "Sandbox",
      description: "Execution boundary",
      currentValue: "workspace",
      options: [
        { value: "workspace", name: "Workspace only", description: "Confine writes to repo", group: "Local" },
        { value: "read_only", name: "Read only", description: "No file writes", group: "Local" },
        { value: "full", name: "Full access", description: "Unrestricted tools", group: "Elevated" },
      ],
    },
  ]);

  // End-to-end boolean toggle through runtimePlan -> session/set_config_option.
  const ctx = { ...baseCtx, tabId: "bool-control" };
  const initOp: Operation = { kind: "initialize" };
  const initPlan = runtimePlan(ctx, initOp);
  runtimeResponse(ctx, initOp, initPlan.state, { kind: "success", value: { protocolVersion: 1, agentCapabilities: {}, authMethods: [] } });
  const openOp: Operation = { kind: "openThread", instructions: "", settings: {} };
  const openPlan = runtimePlan(ctx, openOp);
  const loaded = runtimeResponse(ctx, openOp, openPlan.state, {
    kind: "success",
    value: {
      sessionId: "sess-bool",
      configOptions: [{ id: "web_search", name: "Web search", type: "boolean", currentValue: false }],
    },
  });
  assert.equal(loaded.kind, "complete");
  const bound = { ...ctx, threadId: "sess-bool" };
  const toggleOp: Operation = { kind: "control", threadId: "sess-bool", action: "config", value: { id: "web_search", value: "true" } };
  const togglePlan = runtimePlan(bound, toggleOp);
  assert.deepEqual(togglePlan.action, {
    kind: "request",
    call: {
      method: "session/set_config_option",
      params: { sessionId: "sess-bool", configId: "web_search", type: "boolean", value: true },
    },
  });
  const invalidToggle = runtimePlan(bound, { kind: "control", threadId: "sess-bool", action: "config", value: { id: "web_search", value: "maybe" } });
  assert.equal(invalidToggle.action.kind, "refused");
  assert.deepEqual(
    sessionSettings({ configOptions: [{ id: "web_search", name: "Web search", type: "boolean", currentValue: true }] }).configOptions?.[0],
    { id: "web_search", name: "Web search", kind: "boolean", currentValue: "true", options: [] },
  );
}

// 3. Calm stopReason (max_tokens, max_turn_requests, refusal) vs normal end_turn.
{
  const cases = [
    ["max_tokens", "Stopped at output limit"],
    ["max_turn_requests", "Stopped at turn limit"],
    ["refusal", "Agent refused this request"],
  ] as const;
  for (const [reason, label] of cases) {
    const ctx = { ...baseCtx, threadId: `sess-${reason}` };
    const streamed = reduceStream(ctx, null, [{
      kind: "notification",
      method: "session/update",
      params: { sessionId: ctx.threadId, update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "Partial" } } },
    }], { prompt: 1, replay: 0, loading: false });
    const settled = settleStream(ctx, streamed.state, reason);
    const stopItem = settled.events.find(event => event.kind === "chatItem" && event.item.systemSubtype === "turn_stop_reason");
    assert.ok(stopItem && stopItem.kind === "chatItem", `${reason} emits turn_stop_reason row`);
    assert.deepEqual(stopItem.item.toolInput, { stopReason: reason });
    assert.equal(stopItem.item.content, label);
    assert.deepEqual(
      settled.events.find(event => event.kind === "turnState"),
      { kind: "turnState", threadId: ctx.threadId, turnId: `${ctx.threadId}:turn:1`, state: "idle" },
    );
  }
  const normalCtx = { ...baseCtx, threadId: "sess-end-turn" };
  const normalStream = reduceStream(normalCtx, null, [], { prompt: 1, replay: 0, loading: false });
  const normalSettled = settleStream(normalCtx, normalStream.state, "end_turn");
  assert.equal(normalSettled.events.some(event => event.kind === "chatItem" && event.item.systemSubtype === "turn_stop_reason"), false);
}

// 4. Under-the-hood HTTP MCP selection and automatic stdio fallback.
{
  assert.equal(supportsHttpMcp({ mcpCapabilities: { http: true } }), true);
  assert.equal(supportsHttpMcp({ mcpCapabilities: { http: false } }), false);
  assert.equal(supportsHttpMcp({}), false);

  const stdioOnlyCtx: RuntimeContext = {
    ...baseCtx,
    tabId: "mcp-stdio",
    hostMcp: { name: "domios", command: "/bin/domios", args: ["mcp", "serve"], env: { B: "2", A: "1" } },
  };
  assert.deepEqual(selectHostMcpServers(stdioOnlyCtx, { mcpCapabilities: { http: true } }), {
    transport: "stdio",
    servers: [{ name: "domios", command: "/bin/domios", args: ["mcp", "serve"], env: [{ name: "A", value: "1" }, { name: "B", value: "2" }] }],
  });

  const httpCtx: RuntimeContext = {
    ...stdioOnlyCtx,
    tabId: "mcp-http",
    hostMcp: {
      ...stdioOnlyCtx.hostMcp!,
      http: { url: "http://127.0.0.1:7686/api/mcp", headers: { "X-Tab": "own", Authorization: "Bearer tok" } },
    },
  };
  assert.deepEqual(selectHostMcpServers(httpCtx, { mcpCapabilities: { http: true } }), {
    transport: "http",
    servers: [{
      type: "http",
      name: "domios",
      url: "http://127.0.0.1:7686/api/mcp",
      headers: [{ name: "Authorization", value: "Bearer tok" }, { name: "X-Tab", value: "own" }],
    }],
  });
  assert.equal(selectHostMcpServers(httpCtx, { mcpCapabilities: { http: false } }).transport, "stdio");

  // Runtime falls back from HTTP MCP to stdio on session/new error before dropping MCP altogether.
  const initOp: Operation = { kind: "initialize" };
  const initPlan = runtimePlan(httpCtx, initOp);
  runtimeResponse(httpCtx, initOp, initPlan.state, {
    kind: "success",
    value: { protocolVersion: 1, agentCapabilities: { mcpCapabilities: { http: true } }, authMethods: [] },
  });
  const openOp: Operation = { kind: "openThread", instructions: "", settings: {} };
  const openPlan = runtimePlan(httpCtx, openOp);
  assert.equal(openPlan.action.kind, "request");
  if (openPlan.action.kind === "request") {
    assert.deepEqual((openPlan.action.call.params as { mcpServers: JsonValue[] }).mcpServers[0], {
      type: "http",
      name: "domios",
      url: "http://127.0.0.1:7686/api/mcp",
      headers: [{ name: "Authorization", value: "Bearer tok" }, { name: "X-Tab", value: "own" }],
    });
  }
  const fallbackToStdio = runtimeResponse(httpCtx, openOp, openPlan.state, { kind: "error", error: { code: -32603, message: "http mcp rejected" } });
  assert.equal(fallbackToStdio.kind, "continue");
  if (fallbackToStdio.kind === "continue" && fallbackToStdio.plan.action.kind === "request") {
    assert.deepEqual((fallbackToStdio.plan.action.call.params as { mcpServers: JsonValue[] }).mcpServers[0], {
      name: "domios",
      command: "/bin/domios",
      args: ["mcp", "serve"],
      env: [{ name: "A", value: "1" }, { name: "B", value: "2" }],
    });
  }
}

console.log("✓ ACP UX polish: auto-title, boolean/grouped configOptions, calm stopReason, and HTTP MCP fallback");
