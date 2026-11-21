import assert from "node:assert/strict";
import fs from "node:fs";
import type { AgentEvent, Operation, RpcError, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";
import {
  classifyPromptQuotaError,
  parseExactDurationMs,
  TEXT_RESET_SAFETY_MARGIN_MS,
} from "../src/runtimeQuota";
import { runtimePlan, runtimeReduce, runtimeResponse } from "../src/runtime";
import { runtimeJson } from "./runtimeJson";
import { assertRuntimePlan } from "./runtimeAuthority";

const NOW = 1_795_280_000_000;
const SESSION = "session-quota-fixture";
const baseContext = {
  epoch: 1,
  cwd: "/work/project",
  hostVersion: "1.21.1",
  bindingNonce: "nonce-quota",
  tabId: "",
  settings: {},
  providerSettings: {},
  claimedSends: [],
  activeTurns: [],
  threadParents: [],
} as unknown as RuntimeContext;

function openSession(tabId: string): RuntimeContext {
  const unbound = { ...baseContext, tabId };
  const init: Operation = { kind: "initialize" };
  const initPlan = runtimeJson(runtimePlan(unbound, init), "runtimePlan");
  assertRuntimePlan(unbound, init, initPlan);
  runtimeJson(
    runtimeResponse(unbound, init, initPlan.state, {
      kind: "success",
      value: { protocolVersion: 1, agentCapabilities: {}, authMethods: [] },
    }),
    "runtimeResponse",
  );
  const open: Operation = { kind: "openThread", instructions: "", settings: {} };
  const openPlan = runtimeJson(runtimePlan(unbound, open), "runtimePlan");
  assertRuntimePlan(unbound, open, openPlan);
  runtimeJson(
    runtimeResponse(unbound, open, openPlan.state, {
      kind: "success",
      value: { sessionId: SESSION },
    }),
    "runtimeResponse",
  );
  return { ...unbound, threadId: SESSION };
}

// 1. Structured error.data extraction (authoritative fields first)
{
  const expectedAt = NOW + 600_000;
  for (const field of ["resetsAtMs", "resetAtMs", "retryAtMs", "resets_at_ms", "reset_at_ms", "retry_at_ms"]) {
    const signal = classifyPromptQuotaError(
      { code: -32603, message: "Rate limit exceeded", data: { [field]: expectedAt } },
      NOW,
    );
    assert.deepEqual(
      signal,
      { retryable: false, retryableKind: "rate_limited_until", retryAtMs: expectedAt },
      `structured ${field}`,
    );
  }

  const isoTarget = new Date(NOW + 900_000).toISOString();
  for (const field of ["resetsAt", "resetAt", "resetTime", "resets_at", "reset_at", "reset_time"]) {
    const signal = classifyPromptQuotaError(
      { code: 429, message: "Too many requests", data: { [field]: isoTarget } },
      NOW,
    );
    assert.deepEqual(
      signal,
      { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 900_000 },
      `ISO ${field}`,
    );
  }

  // Unix epoch seconds and milliseconds in resetsAt
  const epochSecs = Math.floor((NOW + 1_200_000) / 1000);
  assert.deepEqual(
    classifyPromptQuotaError({ code: -32603, message: "quota", data: { resetsAt: epochSecs } }, NOW),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: epochSecs * 1000 },
  );
  assert.deepEqual(
    classifyPromptQuotaError({ code: -32603, message: "quota", data: { resetsAt: NOW + 1_200_000 } }, NOW),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 1_200_000 },
  );

  // retryAfterMs and retryAfter / retry_after / retryAfterSeconds / retry_after_secs
  assert.deepEqual(
    classifyPromptQuotaError({ code: -32603, message: "rate limited", data: { retryAfterMs: 45_000 } }, NOW),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 45_000 },
  );
  for (const field of ["retryAfter", "retry_after", "retryAfterSeconds", "retry_after_secs"]) {
    assert.deepEqual(
      classifyPromptQuotaError({ code: -32603, message: "rate limited", data: { [field]: 120 } }, NOW),
      { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 120_000 },
      `structured ${field}`,
    );
  }
  assert.deepEqual(
    classifyPromptQuotaError({ code: -32603, message: "rate limited", data: { retryAfter: "51m16s" } }, NOW),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + (51 * 60 + 16) * 1000 },
  );

  // Google RPC RetryInfo (details[].retryDelay as string duration or protobuf object)
  assert.deepEqual(
    classifyPromptQuotaError(
      {
        code: -32603,
        message: "RESOURCE_EXHAUSTED",
        data: {
          error: {
            code: 429,
            status: "RESOURCE_EXHAUSTED",
            details: [
              { "@type": "type.googleapis.com/google.rpc.RetryInfo", retryDelay: "3076s" },
            ],
          },
        },
      },
      NOW,
    ),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 3_076_000 },
  );
  assert.deepEqual(
    classifyPromptQuotaError(
      {
        code: -32603,
        message: "Quota reached",
        data: {
          details: [
            { "@type": "type.googleapis.com/google.rpc.RetryInfo", retryDelay: "51m16s" },
          ],
        },
      },
      NOW,
    ),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 3_076_000 },
  );
  assert.deepEqual(
    classifyPromptQuotaError(
      {
        code: -32603,
        message: "Quota reached",
        data: {
          details: [
            { "@type": "type.googleapis.com/google.rpc.RetryInfo", retryDelay: { seconds: 90, nanos: 500_000_000 } },
          ],
        },
      },
      NOW,
    ),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 90_500 },
  );

  // Stringified JSON error.data
  assert.deepEqual(
    classifyPromptQuotaError(
      {
        code: -32603,
        message: "Provider error",
        data: JSON.stringify({ status: "RESOURCE_EXHAUSTED", retryAfter: 300 }),
      },
      NOW,
    ),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 300_000 },
  );

  // Structured metadata wins over text fallback when both are present
  assert.deepEqual(
    classifyPromptQuotaError(
      {
        code: -32603,
        message: "Individual quota reached. Resets in 51m16s.",
        data: { resetsAtMs: NOW + 1_000_000 },
      },
      NOW,
    ),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 1_000_000 },
  );
  console.log("✓ ACP structured error.data quota/rate-limit reset extraction passed");
}

// 2. Bounded text fallback on explicit quota/rate-limit refusals with exact reset durations
{
  const antigravity = classifyPromptQuotaError(
    {
      code: -32603,
      message: "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 51m16s.",
    },
    NOW,
  );
  assert.deepEqual(antigravity, {
    retryable: false,
    retryableKind: "rate_limited_until",
    retryAtMs: NOW + (51 * 60 + 16) * 1000 + TEXT_RESET_SAFETY_MARGIN_MS,
  });

  const compound = classifyPromptQuotaError(
    { code: -32603, message: "Usage limit exceeded. Resets in 1h2m3s." },
    NOW,
  );
  assert.deepEqual(compound, {
    retryable: false,
    retryableKind: "rate_limited_until",
    retryAtMs: NOW + (3600 + 120 + 3) * 1000 + TEXT_RESET_SAFETY_MARGIN_MS,
  });

  const seconds = classifyPromptQuotaError(
    { code: -32603, message: "Rate limit exceeded. Please try again in 45s." },
    NOW,
  );
  assert.deepEqual(seconds, {
    retryable: false,
    retryableKind: "rate_limited_until",
    retryAtMs: NOW + 45_000 + TEXT_RESET_SAFETY_MARGIN_MS,
  });

  const words = classifyPromptQuotaError(
    { code: -32603, message: "Too many requests. Try again in 2 hours 15 minutes." },
    NOW,
  );
  assert.deepEqual(words, {
    retryable: false,
    retryableKind: "rate_limited_until",
    retryAtMs: NOW + (2 * 3600 + 15 * 60) * 1000 + TEXT_RESET_SAFETY_MARGIN_MS,
  });

  // Explicit ISO timestamp in quota text
  const isoAt = new Date(NOW + 1_800_000).toISOString();
  assert.deepEqual(
    classifyPromptQuotaError(
      { code: -32603, message: `Quota exhausted. Resets at ${isoAt}.` },
      NOW,
    ),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 1_800_000 },
  );

  // Structured 429 / RESOURCE_EXHAUSTED authorizes text duration extraction even on terse message
  assert.deepEqual(
    classifyPromptQuotaError(
      { code: 429, message: "Please try again in 30s." },
      NOW,
    ),
    { retryable: false, retryableKind: "rate_limited_until", retryAtMs: NOW + 30_000 + TEXT_RESET_SAFETY_MARGIN_MS },
  );
  console.log("✓ ACP explicit quota text fallback extracts exact durations with safety margin");
}

// 3. Strict negative cases: non-quota errors, quota without reset time, invalid/absurd/past/auth errors
{
  const rejected: [string, RpcError][] = [
    ["non-quota with duration", { code: -32603, message: "Build failed; resets in 51m16s." }],
    ["timeout with try again", { code: -32603, message: "Upstream connection timed out. Try again in 45s." }],
    ["busy refusal", { code: -32602, message: "Invalid params: a prompt is already in flight for this session" }],
    ["quota without reset time", { code: -32603, message: "Individual quota reached. Please upgrade your subscription." }],
    ["rate limit with vague timing", { code: -32603, message: "Rate limit exceeded. Please try again later." }],
    ["quota with vague soon", { code: 429, message: "Quota exceeded. Resets soon." }],
    ["zero duration text", { code: -32603, message: "Individual quota reached. Resets in 0s." }],
    ["negative duration text", { code: -32603, message: "Individual quota reached. Resets in -5m." }],
    ["malformed unit text", { code: -32603, message: "Individual quota reached. Resets in 51m 16x." }],
    ["duplicate unit text", { code: -32603, message: "Individual quota reached. Resets in 5m 5m." }],
    ["absurd duration text", { code: -32603, message: "Individual quota reached. Resets in 999999h." }],
    ["past structured resetsAtMs", { code: -32603, message: "Quota reached", data: { resetsAtMs: NOW - 1000 } }],
    ["zero structured retryAfter", { code: 429, message: "Rate limited", data: { retryAfter: 0 } }],
    ["absurd structured retryAfter", { code: 429, message: "Rate limited", data: { retryAfter: 90 * 86400 } }],
    ["ACP auth error -32000", { code: -32000, message: "Individual quota reached. Resets in 51m16s.", data: { retryAfter: 60 } }],
    ["HTTP 401 with retryAfter", { code: -32603, message: "Unauthorized", data: { status: 401, retryAfter: 60 } }],
    ["HTTP 403 with resetsAtMs", { code: -32603, message: "Forbidden", data: { statusCode: 403, resetsAtMs: NOW + 60_000 } }],
    ["UNAUTHENTICATED with RetryInfo", { code: -32603, message: "Unauthenticated", data: { status: "UNAUTHENTICATED", details: [{ "@type": "type.googleapis.com/google.rpc.RetryInfo", retryDelay: "60s" }] } }],
  ];
  for (const [label, error] of rejected) {
    assert.equal(classifyPromptQuotaError(error, NOW), null, `must reject: ${label}`);
  }
  assert.equal(parseExactDurationMs("0s"), null);
  assert.equal(parseExactDurationMs("5m 10m"), null);
  assert.equal(parseExactDurationMs("10s 5m"), null);
  assert.equal(parseExactDurationMs("51m16x"), null);
  console.log("✓ ACP quota extractor strictly rejects non-quota, vague, zero, absurd, past, and auth errors");
}

// 4. End-to-end runtimePlan -> runtimeResponse lifecycle integration
{
  const bound = openSession("quota-active-prompt");
  const submit: Operation = {
    kind: "submitTurn",
    threadId: SESSION,
    sendId: "send-quota-1",
    input: { text: "continue work", attachments: [] },
    settings: {},
  };
  const plan = runtimeJson(runtimePlan(bound, submit), "runtimePlan");
  assertRuntimePlan(bound, submit, plan);
  const resetAtMs = Date.now() + 3_076_000;
  const message = "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 51m16s.";
  const step = runtimeJson(
    runtimeResponse(bound, submit, plan.state, {
      kind: "error",
      error: { code: -32603, message, data: { resetsAtMs: resetAtMs } },
    }),
    "runtimeResponse",
  );
  assert.equal(step.kind, "complete");
  if (step.kind !== "complete") throw new Error("expected complete step on deterministic quota error");
  assert.deepEqual(step.result, {
    kind: "turnAccepted",
    threadId: SESSION,
    sendId: "send-quota-1",
    turnId: `${SESSION}:turn:1`,
  });
  const events: AgentEvent[] = step.events ?? [];
  assert.equal(events.some((event) => event.kind === "sendEvidence"), false, "error reply never fabricates sendEvidence");
  const errorEvent = events.find((event) => event.kind === "chatItem");
  assert.ok(errorEvent && errorEvent.kind === "chatItem");
  assert.equal(errorEvent.item.id, `${SESSION}:turn:1:error`);
  assert.equal(errorEvent.item.type, "system");
  assert.equal(errorEvent.item.systemSubtype, "api_error");
  assert.equal(errorEvent.item.toolFlowId, `${SESSION}:turn:1`);
  assert.equal(errorEvent.item.content, message);
  assert.deepEqual(errorEvent.item.toolInput, {
    retryable: false,
    retryableKind: "rate_limited_until",
    retryAtMs: resetAtMs,
  });
  const idleEvent = events.find((event) => event.kind === "turnState");
  assert.deepEqual(idleEvent, {
    kind: "turnState",
    threadId: SESSION,
    turnId: `${SESSION}:turn:1`,
    state: "idle",
    detail: "failed",
  });
  assert.ok(events.indexOf(errorEvent) < events.indexOf(idleEvent!), "api_error must precede TurnState Idle(Failed)");

  // Follow-up delayed "continue" turn starts cleanly at ordinal 2 and completes normally
  const retrySubmit: Operation = {
    kind: "submitTurn",
    threadId: SESSION,
    sendId: "send-continue-2",
    input: { text: "continue", attachments: [] },
    settings: {},
  };
  const retryPlan = runtimeJson(runtimePlan(bound, retrySubmit), "runtimePlan");
  assert.equal(retryPlan.action.kind, "request");
  if (retryPlan.action.kind === "request") {
    assert.equal(retryPlan.action.call.target?.turnId, `${SESSION}:turn:2`);
  }
  const retryStep = runtimeJson(
    runtimeResponse(bound, retrySubmit, retryPlan.state, {
      kind: "success",
      value: { stopReason: "end_turn" },
    }),
    "runtimeResponse",
  );
  assert.equal(retryStep.kind, "complete");
  if (retryStep.kind === "complete") {
    assert.deepEqual(
      (retryStep.events ?? []).filter((event) => event.kind === "turnState"),
      [{ kind: "turnState", threadId: SESSION, turnId: `${SESSION}:turn:2`, state: "idle" }],
    );
  }
  console.log("✓ ACP runtimeResponse emits api_error + TurnState Idle(Failed) and cleanly admits follow-up continue");
}

// 5. Mid-turn quota error settles running tool calls before emitting api_error + TurnState Idle(Failed)
{
  const bound = openSession("quota-mid-turn");
  const submit: Operation = {
    kind: "submitTurn",
    threadId: SESSION,
    sendId: "send-mid",
    input: { text: "run tool", attachments: [] },
    settings: {},
  };
  const plan = runtimeJson(runtimePlan(bound, submit), "runtimePlan");
  const runningFrame: RuntimeFrame = {
    kind: "notification",
    method: "session/update",
    params: {
      sessionId: SESSION,
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "tool-1",
        title: "Read file",
        kind: "read",
        status: "in_progress",
      },
    },
  };
  runtimeJson(runtimeReduce(bound, null, [runningFrame]), "runtimeReduce");
  const step = runtimeJson(
    runtimeResponse(bound, submit, plan.state, {
      kind: "error",
      error: {
        code: -32603,
        message: "Individual quota reached. Resets in 51m16s.",
      },
    }),
    "runtimeResponse",
  );
  assert.equal(step.kind, "complete");
  if (step.kind !== "complete") throw new Error("expected complete step");
  const events = step.events ?? [];
  assert.ok(events.some((event) => event.kind === "chatItemPatch" && event.item.toolId === "tool-1" && event.item.toolStatus === "interrupted"));
  const errorEvent = events.find((event) => event.kind === "chatItem" && event.item.systemSubtype === "api_error");
  assert.ok(errorEvent && errorEvent.kind === "chatItem");
  const signal = errorEvent.item.toolInput as { retryableKind?: string; retryAtMs?: number; retryable?: boolean };
  assert.equal(signal.retryableKind, "rate_limited_until");
  assert.equal(signal.retryable, false);
  assert.ok(typeof signal.retryAtMs === "number" && signal.retryAtMs > Date.now() + 3_000_000);
  assert.deepEqual(events[events.length - 1], {
    kind: "turnState",
    threadId: SESSION,
    turnId: `${SESSION}:turn:1`,
    state: "idle",
    detail: "failed",
  });
  console.log("✓ ACP mid-turn quota error interrupts running tools before api_error and TurnState Idle(Failed)");
}

// 6. Non-quota errors, quota without reset time, and -32000 auth errors stay refused without emitting rate_limited_until
{
  const bound = openSession("quota-refused-fallback");
  for (const [label, error, expectedCode] of [
    ["unclassified error", { code: -32603, message: "Internal failure" }, "provider_refused"],
    ["quota without reset time", { code: 429, message: "Individual quota reached. Please upgrade." }, "provider_refused"],
    ["auth error -32000", { code: -32000, message: "Individual quota reached. Resets in 51m16s." }, "authentication_required"],
  ] as const) {
    const submit: Operation = {
      kind: "submitTurn",
      threadId: SESSION,
      sendId: `send-${label}`,
      input: { text: label, attachments: [] },
      settings: {},
    };
    const plan = runtimeJson(runtimePlan(bound, submit), "runtimePlan");
    const step = runtimeJson(runtimeResponse(bound, submit, plan.state, { kind: "error", error }), "runtimeResponse");
    assert.equal(step.kind, "refused", label);
    if (step.kind === "refused") {
      assert.equal(step.reason.code, expectedCode, label);
    }
  }
  console.log("✓ ACP non-quota, unparseable quota, and auth errors remain refused without scheduling retry");
}

// 7. ACP usage_update / quota_update extension quota payload maps to rateLimits fail-closed
{
  const bound = openSession("quota-extension-update");
  const isoReset = "2026-11-22T05:00:00Z";
  const goodFrame: RuntimeFrame = {
    kind: "notification",
    method: "session/update",
    params: {
      sessionId: SESSION,
      update: {
        sessionUpdate: "usage_update",
        used: 1200,
        size: 100000,
        _meta: {
          quota: {
            "gemini-pro": { remaining_fraction: 0.4, reset_time: isoReset },
            "disabled-bucket": { remaining_fraction: 0.1, disabled: true },
          },
        },
      },
    },
  };
  const reduced = runtimeJson(runtimeReduce(bound, null, [goodFrame]), "runtimeReduce");
  const rateEvent = reduced.events.find((event) => event.kind === "rateLimits");
  assert.ok(rateEvent && rateEvent.kind === "rateLimits", "valid _meta.quota emits rateLimits");
  assert.deepEqual(rateEvent.rateLimits, [
    { window: "gemini-pro", usedPercent: 60, resetsAt: Date.parse(isoReset) },
  ]);

  const badFrame: RuntimeFrame = {
    kind: "notification",
    method: "session/update",
    params: {
      sessionId: SESSION,
      update: {
        sessionUpdate: "usage_update",
        used: 1500,
        size: 100000,
        quota: { "gemini-pro": { remaining_fraction: 2.5 } },
      },
    },
  };
  const badReduced = runtimeJson(runtimeReduce(bound, reduced.state, [badFrame]), "runtimeReduce");
  assert.equal(badReduced.events.some((event) => event.kind === "rateLimits"), false, "malformed quota fails closed without dropping context usage");
  assert.equal(badReduced.events.some((event) => event.kind === "context"), true);
  console.log("✓ ACP usage_update quota extension projects valid buckets to rateLimits and fails closed on malformed");
}

// 8. QuickJS ES2020 static safety guard for runtimeQuota.ts
{
  const source = fs.readFileSync(new URL("../src/runtimeQuota.ts", import.meta.url), "utf8");
  for (const forbidden of [".at(", "Object.hasOwn(", ".replaceAll(", "console."]) {
    assert.equal(source.includes(forbidden), false, `runtimeQuota.ts must remain ES2020 QuickJS-safe (found ${forbidden})`);
  }
  console.log("✓ ACP runtimeQuota.ts passes QuickJS ES2020 static guard");
}
