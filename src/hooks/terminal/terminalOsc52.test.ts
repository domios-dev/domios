import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  evaluateOsc52ClipboardWrite,
  MAX_OSC52_TEXT_BYTES,
  OSC52_USER_INTENT_WINDOW_MS,
  type Osc52Context,
} from "./terminalOsc52";

function b64(text: string): string {
  return Buffer.from(text, "utf8").toString("base64");
}

function activeContext(overrides: Partial<Osc52Context> = {}): Osc52Context {
  return {
    isRemoteTab: true,
    terminalActive: true,
    replayingHistory: false,
    nowMs: 10_000,
    lastUserIntentAtMs: 9_600,
    ...overrides,
  };
}

describe("OSC 52 clipboard routing and safety", () => {
  it("writes decoded UTF-8 payload to the viewer clipboard on remote and local user-initiated copy", () => {
    const loginUrl = "https://claude.ai/oauth/authorize?code=true&state=opaque_123";
    for (const isRemoteTab of [true, false]) {
      for (const selection of ["c", "", "cp"]) {
        const decision = evaluateOsc52ClipboardWrite(
          `${selection};${b64(loginUrl)}`,
          activeContext({ isRemoteTab }),
        );
        assert.deepEqual(decision, {
          action: "write",
          text: loginUrl,
          target: "viewer_clipboard",
        });
      }
    }

    const unicode = "Привет, Domios! 🚀";
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(`c;${b64(unicode)}`, activeContext()),
      { action: "write", text: unicode, target: "viewer_clipboard" },
    );
  });

  it("always refuses OSC 52 clipboard read queries so viewer clipboard never leaks to remote", () => {
    for (const query of ["c;?", ";?", "p;?", "c; ? "]) {
      assert.deepEqual(
        evaluateOsc52ClipboardWrite(query, activeContext()),
        { action: "ignore", reason: "read_query_forbidden" },
      );
    }
  });

  it("requires an active, non-replaying terminal with recent user intent", () => {
    const payload = `c;${b64("https://claude.ai/oauth/authorize?state=1")}`;

    assert.deepEqual(
      evaluateOsc52ClipboardWrite(payload, activeContext({ terminalActive: false })),
      { action: "ignore", reason: "inactive_terminal" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(payload, activeContext({ replayingHistory: true })),
      { action: "ignore", reason: "replaying_history" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(payload, activeContext({ lastUserIntentAtMs: null })),
      { action: "ignore", reason: "missing_user_intent" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(
        payload,
        activeContext({ nowMs: 20_000, lastUserIntentAtMs: 20_000 - OSC52_USER_INTENT_WINDOW_MS - 1 }),
      ),
      { action: "ignore", reason: "missing_user_intent" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(
        payload,
        activeContext({ nowMs: 10_000, lastUserIntentAtMs: 10_001 }),
      ),
      { action: "ignore", reason: "missing_user_intent" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(
        payload,
        activeContext({ nowMs: 20_000, lastUserIntentAtMs: 20_000 - OSC52_USER_INTENT_WINDOW_MS }),
      ),
      { action: "write", text: "https://claude.ai/oauth/authorize?state=1", target: "viewer_clipboard" },
    );
  });

  it("enforces the same clipboard size limit as local and rejects malformed/unsupported sequences", () => {
    const atLimit = "a".repeat(MAX_OSC52_TEXT_BYTES);
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(`c;${b64(atLimit)}`, activeContext()),
      { action: "write", text: atLimit, target: "viewer_clipboard" },
    );

    const overLimit = "a".repeat(MAX_OSC52_TEXT_BYTES + 1);
    assert.deepEqual(
      evaluateOsc52ClipboardWrite(`c;${b64(overLimit)}`, activeContext()),
      { action: "ignore", reason: "too_large" },
    );

    assert.deepEqual(
      evaluateOsc52ClipboardWrite("no-semicolon", activeContext()),
      { action: "ignore", reason: "malformed" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite("p;aGk=", activeContext()),
      { action: "ignore", reason: "unsupported_selection" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite("x;aGk=", activeContext()),
      { action: "ignore", reason: "unsupported_selection" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite("c;", activeContext()),
      { action: "ignore", reason: "empty" },
    );
    assert.deepEqual(
      evaluateOsc52ClipboardWrite("c;!!!not-base64!!!", activeContext()),
      { action: "ignore", reason: "invalid_base64" },
    );
  });
});
