import assert from "node:assert/strict";
import { latestTurnStopNoticeId } from "./chatTurnTail";
import type { EnrichedChatEntry } from "./chatEnrichment";

const entry = (msg: EnrichedChatEntry["msg"]): EnrichedChatEntry => ({
  msg,
  item: { kind: "message", message: msg },
});

const retryableRemediation = {
  remediation: {
    code: "api_key_expired",
    title: "Stored key has expired",
    message: "Sign in again or remove the stored key.",
    commands: [{ label: "Sign in", command: "provider auth login" }],
    retry: true,
  },
};

assert.equal(
  latestTurnStopNoticeId([
    entry({ id: "u1", type: "user", content: "hi" }),
    entry({ id: "e1", type: "system", systemSubtype: "api_error", content: "expired", toolInput: retryableRemediation }),
    entry({ id: "s1", type: "system", content: "Automatic recovery stopped" }),
  ]),
  "e1",
  "tail retryable api_error remains eligible for Retry even behind trailing system status lines",
);

assert.equal(
  latestTurnStopNoticeId([
    entry({ id: "u1", type: "user", content: "hi" }),
    entry({ id: "e1", type: "system", systemSubtype: "api_error", content: "expired", toolInput: retryableRemediation }),
    entry({ id: "u2", type: "user", content: "Continue" }),
  ]),
  null,
  "a newer user turn clears Retry eligibility for an earlier api_error",
);

assert.equal(
  latestTurnStopNoticeId([
    entry({ id: "u1", type: "user", content: "hi" }),
    entry({ id: "e1", type: "system", systemSubtype: "api_error", content: "generic error" }),
  ]),
  null,
  "plain api_error without retryable remediation does not gain a Retry action",
);

console.log("✓ chatTurnTail actionable api_error Retry gating passed");
