import assert from "node:assert/strict";
import { test } from "node:test";
import type { JsonValue, RuntimeContext } from "@codeterm/plugin-sdk";
import { runtimePlan, runtimeResponse, runtimeReduce } from "../src/runtime";
import { runtimePrompt, runtimeAnswer } from "../src/runtimePrompts";
import { reduceTool } from "../src/runtimeReduceTools";

function ctx(overrides: Partial<RuntimeContext> = {}): RuntimeContext {
  return {
    epoch: 1,
    cwd: "/work/project",
    hostVersion: "1.21.1",
    tabId: "live-ux-tab",
    bindingNonce: "nonce-live-ux",
    settings: {},
    providerSettings: {},
    claimedSends: [],
    activeTurns: [],
    threadParents: [],
    ...overrides,
  };
}

function initSession(
  tabId: string,
  agentCapabilities: Record<string, JsonValue>,
  sessionId = "sess-1",
): RuntimeContext {
  const base = ctx({ tabId });
  const initPlan = runtimePlan(base, { kind: "initialize" });
  const initStep = runtimeResponse(base, { kind: "initialize" }, initPlan.state, {
    kind: "success",
    value: { protocolVersion: 1, agentCapabilities, authMethods: [] },
  });
  assert.equal(initStep.kind, "complete");
  const openPlan = runtimePlan(base, { kind: "openThread", instructions: "", settings: {} });
  const openStep = runtimeResponse(base, { kind: "openThread", instructions: "", settings: {} }, openPlan.state, {
    kind: "success",
    value: { sessionId },
  });
  assert.equal(openStep.kind, "complete");
  return ctx({ tabId, threadId: sessionId });
}

test("session/request_permission attaches structured file_diffs only when diffs exist", () => {
  const context = ctx({ threadId: "sess-diff" });
  const withDiff = runtimePrompt(context, "session/request_permission", {
    sessionId: "sess-diff",
    toolCall: {
      toolCallId: "tc-1",
      title: "Edit app.ts",
      kind: "edit",
      content: [{ type: "diff", path: "src/app.ts", oldText: "const a = 1;\n", newText: "const a = 2;\n" }],
    },
    options: [
      { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
      { optionId: "reject_once", name: "Reject", kind: "reject_once" },
    ],
  });
  assert.ok(withDiff);
  assert.equal(withDiff.fileDiffs?.length, 1);
  assert.equal((withDiff.fileDiffs![0] as Record<string, unknown>).path, "src/app.ts");
  assert.equal((withDiff.fileDiffs![0] as Record<string, unknown>).additions, 1);
  assert.equal((withDiff.fileDiffs![0] as Record<string, unknown>).deletions, 1);

  const withoutDiff = runtimePrompt(context, "session/request_permission", {
    sessionId: "sess-diff",
    toolCall: { toolCallId: "tc-2", title: "Run tests", kind: "execute" },
    options: [{ optionId: "allow_once", name: "Allow once", kind: "allow_once" }],
  });
  assert.ok(withoutDiff);
  assert.equal(withoutDiff.fileDiffs, undefined);
});

test("elicitation/create supports form mode schema validation and url/OAuth mode safely", () => {
  const context = ctx({ threadId: "sess-elicit" });
  const formParams: JsonValue = {
    sessionId: "sess-elicit",
    mode: "form",
    message: "Configure target",
    requestedSchema: {
      type: "object",
      required: ["env", "retries"],
      properties: {
        env: { type: "string", title: "Environment", enum: ["staging", "prod"] },
        retries: { type: "integer", title: "Retries", minimum: 1, maximum: 5 },
        dryRun: { type: "boolean", title: "Dry run", default: false },
      },
    },
  };
  const formReq = runtimePrompt(context, "elicitation/create", formParams);
  assert.ok(formReq);
  assert.deepEqual(formReq.prompt.options, []);
  assert.equal((formReq.elicitation as Record<string, unknown>).mode, "form");
  assert.deepEqual(
    runtimeAnswer(context, "elicitation/create", formParams, 7, {
      kind: "text",
      text: JSON.stringify({ action: "accept", content: { env: "staging", retries: 3, dryRun: true } }),
    }),
    { action: "accept", content: { env: "staging", retries: 3, dryRun: true } },
  );
  assert.throws(() =>
    runtimeAnswer(context, "elicitation/create", formParams, 7, {
      kind: "text",
      text: JSON.stringify({ action: "accept", content: { env: "invalid", retries: 3 } }),
    }),
  );
  assert.deepEqual(
    runtimeAnswer(context, "elicitation/create", formParams, 7, { kind: "cancel" }),
    { action: "cancel" },
  );

  const urlParams: JsonValue = {
    sessionId: "sess-elicit",
    mode: "url",
    message: "Sign in to GitHub",
    url: "https://github.com/login/oauth/authorize?client_id=abc",
  };
  const urlReq = runtimePrompt(context, "elicitation/create", urlParams);
  assert.ok(urlReq);
  assert.equal((urlReq.elicitation as Record<string, unknown>).host, "github.com");
  assert.deepEqual(
    runtimeAnswer(context, "elicitation/create", urlParams, 8, { kind: "choice", values: ["accept"] }),
    { action: "accept" },
  );
  assert.equal(
    runtimePrompt(context, "elicitation/create", { ...urlParams, url: "javascript:alert(1)" }),
    undefined,
  );
  assert.equal(
    runtimePrompt(context, "elicitation/create", { ...urlParams, url: "http://evil.example.com/auth" }),
    undefined,
  );
  assert.ok(
    runtimePrompt(context, "elicitation/create", { ...urlParams, url: "http://localhost:8080/callback" }),
  );
});

test("reduceTool maps all 9 ACP ToolKinds, clickable locations, terminal content, and rich content blocks", () => {
  const expected: Record<string, string> = {
    execute: "command",
    edit: "edit",
    delete: "edit",
    move: "edit",
    read: "read",
    search: "search",
    fetch: "fetch",
    think: "generic",
    switch_mode: "generic",
  };
  for (const [acpKind, domiosKind] of Object.entries(expected)) {
    const row = reduceTool({ toolCallId: `id-${acpKind}`, kind: acpKind, title: acpKind }, undefined, 1);
    assert.equal(row?.toolKind, domiosKind, `kind ${acpKind}`);
  }

  const termRow = reduceTool({
    toolCallId: "tc-term",
    title: "Run vitest",
    status: "in_progress",
    locations: [{ path: "/work/project/src/a.ts", line: 42 }, { path: "/work/project/src/b.ts" }],
    content: [
      { type: "terminal", terminalId: "term-99", output: "running suite...\n" },
      { type: "content", content: { type: "resource_link", uri: "file:///work/project/README.md", name: "README.md" } },
      { type: "content", content: { type: "image", mimeType: "image/png", data: "iVBORw0KGgo=" } },
    ],
  }, undefined, 2);
  assert.ok(termRow);
  assert.equal(termRow.toolKind, "command");
  assert.equal(termRow.toolBackgroundTaskId, "term-99");
  assert.ok(termRow.toolResult?.includes("running suite..."));
  assert.ok(termRow.toolResult?.includes("data:image/png;base64,iVBORw0KGgo="));
  const locations = (termRow as Record<string, unknown>).toolLocations as Array<{ path: string; line?: number }>;
  assert.deepEqual(locations, [
    { path: "/work/project/src/a.ts", line: 42 },
    { path: "/work/project/src/b.ts" },
    { path: "/work/project/README.md" },
  ]);
});

test("runtimeReduce surfaces image, resource_link, resource, and audio blocks in agent_message_chunk", () => {
  const context = initSession("live-media-tab", {}, "sess-media");
  const reduction = runtimeReduce(context, null, [
    {
      kind: "notification",
      method: "session/update",
      params: {
        sessionId: "sess-media",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "resource_link", uri: "file:///work/project/src/index.ts", name: "index.ts" },
        },
      },
    },
    {
      kind: "notification",
      method: "session/update",
      params: {
        sessionId: "sess-media",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "image", mimeType: "image/png", data: "AQID" },
        },
      },
    },
    {
      kind: "notification",
      method: "session/update",
      params: {
        sessionId: "sess-media",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "audio", mimeType: "audio/mpeg" },
        },
      },
    },
  ]);
  const items = reduction.events.filter((e) => e.kind === "chatItem").map((e) => e.item);
  assert.ok(items.some((item) => item.type === "assistant" && item.content.includes("[index.ts](/work/project/src/index.ts)")));
  assert.ok(items.some((item) => item.type === "image" && item.imageData === "AQID" && item.imageMimeType === "image/png"));
  assert.ok(items.some((item) => item.type === "assistant" && item.content.includes("[Audio (audio/mpeg)]")));
});

test("submitTurn sends resource_link, embedded resource, and capability-gated audio blocks", () => {
  const context = initSession("live-attach-tab", {
    promptCapabilities: { image: true, audio: true, embeddedContext: true },
  }, "sess-attach");
  const plan = runtimePlan(context, {
    kind: "submitTurn",
    threadId: "sess-attach",
    sendId: "send-1",
    input: {
      text: 'Check <domios type="attachment" kind="file" path="/work/project/notes.md" />',
      attachments: [
        { id: "a1", mimeType: "audio/webm", data: "T2dn", path: "/tmp/voice.webm" },
        { id: "f1", mimeType: "text/plain", data: "aGVsbG8=", path: "/work/project/data.txt" },
      ],
    },
    settings: {},
  });
  assert.equal(plan.action.kind, "request");
  if (plan.action.kind !== "request") return;
  const prompt = (plan.action.call.params as Record<string, unknown>).prompt as Array<Record<string, unknown>>;
  assert.ok(prompt.some((part) => part.type === "audio" && part.mimeType === "audio/webm" && part.data === "T2dn"));
  assert.ok(prompt.some((part) => part.type === "resource"));
  assert.ok(prompt.some((part) => part.type === "resource_link" && part.name === "notes.md"));

  const noAudio = initSession("live-no-audio-tab", {
    promptCapabilities: { image: true, audio: false, embeddedContext: false },
  }, "sess-no-audio");
  const refused = runtimePlan(noAudio, {
    kind: "submitTurn",
    threadId: "sess-no-audio",
    sendId: "send-2",
    input: { text: "Listen", attachments: [{ id: "a2", mimeType: "audio/webm", data: "T2dn" }] },
    settings: {},
  });
  assert.equal(refused.action.kind, "refused");
});

test("capability-gated session/fork, session/list, and session/close honor agentCapabilities", () => {
  const unadvertised = ctx({ tabId: "no-caps-tab" });
  const init1 = runtimePlan(unadvertised, { kind: "initialize" });
  const step1 = runtimeResponse(unadvertised, { kind: "initialize" }, init1.state, {
    kind: "success",
    value: { protocolVersion: 1, agentCapabilities: { sessionCapabilities: { resume: {} } }, authMethods: [] },
  });
  assert.equal(step1.kind, "complete");
  if (step1.kind === "complete") {
    assert.deepEqual(step1.result, { kind: "initialized" });
  }
  assert.equal(
    runtimePlan(unadvertised, { kind: "forkThread", sourceThreadId: "s-0", turnId: "t-1" }).action.kind,
    "refused",
  );
  assert.equal(
    runtimePlan(unadvertised, { kind: "control", action: "listSessions", value: {} }).action.kind,
    "refused",
  );

  const advertised = ctx({ tabId: "caps-tab" });
  const init2 = runtimePlan(advertised, { kind: "initialize" });
  const step2 = runtimeResponse(advertised, { kind: "initialize" }, init2.state, {
    kind: "success",
    value: {
      protocolVersion: 1,
      agentCapabilities: { sessionCapabilities: { fork: {}, list: {}, close: {} } },
      authMethods: [],
    },
  });
  assert.equal(step2.kind, "complete");
  if (step2.kind === "complete" && step2.result.kind === "initialized") {
    assert.deepEqual(step2.result.sessionCapabilities, { fork: true, list: true, close: true });
  }

  const listPlan = runtimePlan(advertised, { kind: "control", action: "listSessions", value: { cwd: "/work/project" } });
  assert.equal(listPlan.action.kind, "request");
  const listStep = runtimeResponse(advertised, { kind: "control", action: "listSessions", value: {} }, listPlan.state, {
    kind: "success",
    value: {
      sessions: [
        { sessionId: "acp-s1", cwd: "/work/project", title: "Fix auth", updatedAt: "2026-11-21T20:00:00Z" },
        { sessionId: "acp-win-1", cwd: "/c:\\Developer\\codeterm\\codeterm\\", title: "Windows URI cwd", updatedAt: "2026-11-21T21:00:00Z" },
      ],
    },
  });
  assert.equal(listStep.kind, "complete");
  if (listStep.kind === "complete" && listStep.result.kind === "sessionsListed") {
    assert.equal(listStep.result.sessions.length, 2);
    assert.equal(listStep.result.sessions[0].title, "Fix auth");
    assert.equal(listStep.result.sessions[1].cwd, "C:/Developer/codeterm/codeterm");
  }

  const forkPlan = runtimePlan(advertised, { kind: "forkThread", sourceThreadId: "acp-s1", turnId: "acp-s1:turn:1" });
  assert.equal(forkPlan.action.kind, "request");
  if (forkPlan.action.kind === "request") {
    assert.equal(forkPlan.action.call.method, "session/fork");
  }
  const forkStep = runtimeResponse(advertised, { kind: "forkThread", sourceThreadId: "acp-s1", turnId: "acp-s1:turn:1" }, forkPlan.state, {
    kind: "success",
    value: { sessionId: "acp-s2" },
  });
  assert.equal(forkStep.kind, "complete");
  if (forkStep.kind === "complete" && forkStep.result.kind === "threadForked") {
    assert.equal(forkStep.result.threadId, "acp-s2");
  }

  const bound = ctx({ tabId: "caps-tab", threadId: "acp-s2" });
  const closePlan = runtimePlan(bound, { kind: "control", threadId: "acp-s2", action: "closeSession", value: {} });
  assert.equal(closePlan.action.kind, "request");
  if (closePlan.action.kind === "request") {
    assert.equal(closePlan.action.call.method, "session/close");
  }
  const closeStep = runtimeResponse(bound, { kind: "control", threadId: "acp-s2", action: "closeSession", value: {} }, closePlan.state, {
    kind: "success",
    value: {},
  });
  assert.equal(closeStep.kind, "complete");
});
