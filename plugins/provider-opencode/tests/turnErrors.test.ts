import { ioTestExport } from "../../provider-shared/tests/hostIo";
// @vitest-environment node

import { describe, expect, it } from "vitest";
import { MODEL_REJECTION_QUERY, parseOpencodeModels, sessionModelIdentity } from "../src/helpers";
import { classifyOpencodeConsoleKeyError, rejectedModelIds, sessionObservations, turnError, unavailableModelError } from "../src/turnErrors";

const error = { role: "assistant", providerID: "opencode", modelID: "exo-free",
  error: { name: "APIError", data: { message: "Model exo-free has been deprecated", statusCode: 410, isRetryable: false } } };

describe("OpenCode assistant errors", () => {
  it("classifies the non-retryable 410 and preserves its visible message", () => {
    expect(sessionObservations(error)?.api_error).toBe("other");
    expect(turnError(error)?.data?.message).toBe("Model exo-free has been deprecated");
    expect(unavailableModelError(error)).toBe(true);
  });
  it("does not quarantine transient failures or a non-model request error", () => {
    expect(unavailableModelError({ ...error, error: { name: "APIError", data: { ...error.error.data, isRetryable: true } } })).toBe(false);
    expect(unavailableModelError({ ...error, error: { name: "APIError", data: { statusCode: 400, isRetryable: false, message: "Invalid request" } } })).toBe(false);
    expect(sessionObservations({ ...error, role: "user" })).toBeNull();
  });
  it("classifies typed insufficient balance errors on assistant messages", () => {
    const body = { error: { type: "insufficient_balance_error", message: "Insufficient Balance" } };
    const quota = { role: "assistant", error: { type: "provider.quota", status: 402 } };
    expect(turnError(quota)).toEqual(quota.error);
    expect(sessionObservations(quota)?.api_error).toBe("usage_limit");
    expect(sessionObservations({ ...quota, error: { ...quota.error, body } })?.api_error).toBe("usage_limit");
    expect(sessionObservations({ ...quota, error: { body } })?.api_error).toBe("usage_limit");
    expect(sessionObservations({ ...quota, role: "user" })).toBeNull();
  });
  it("uses the most recent completed outcome for each model", () => {
    const success = { role: "assistant", time: { completed: 50 } };
    expect([...rejectedModelIds([{ model: "opencode/exo-free", message: error }, { model: "opencode/good", message: success }])]).toEqual(["opencode/exo-free"]);
    expect([...rejectedModelIds([{ model: "opencode/exo-free", message: success }, { model: "opencode/exo-free", message: error }])]).toEqual([]);
  });
  it("filters live deprecated and unavailable models while retaining unknown availability", () => {
    expect(parseOpencodeModels('opencode/exo-free\n{"status":"deprecated"}\nvendor/off\n{"available":false}\nvendor/unknown\n{"name":"Unknown"}')?.map((model) => model.id)).toEqual(["vendor/unknown"]);
    expect(parseOpencodeModels('opencode/exo-free\n{"status":"deprecated"}')).toEqual([]);
  });
  it("classifies expired and invalid OpenCode Console keys into actionable typed failures", () => {
    const expiredBody = JSON.stringify({
      error: {
        type: "authentication_error",
        code: "api_key_expired",
        message: "This API key has expired. Create a new API key in OpenCode Console and replace it in your client or secret store. Signing in again will not renew this key. Manage your API keys at https://opencode.ai/console/account#api-keys.",
      },
    });
    const v2Expired = {
      type: "provider.auth",
      status: 401,
      message: "This API key has expired. Create a new API key in OpenCode Console and replace it in your client or secret store.",
      response: { body: expiredBody },
    };
    const remediation = classifyOpencodeConsoleKeyError(v2Expired, "opencode/beam-free");
    expect(remediation).toMatchObject({
      code: "api_key_expired",
      title: "Stored OpenCode Console key has expired",
      retry: true,
      commands: [
        { command: "opencode auth login" },
        { command: "opencode auth logout" },
      ],
    });
    expect(remediation?.message).toContain("opencode auth login");
    expect(remediation?.message).toContain("opencode auth logout");
    expect(remediation?.rawDetail).toContain("This API key has expired");

    const v2Message = {
      type: "assistant",
      model: { providerID: "opencode", id: "beam-free" },
      finish: "error",
      error: v2Expired,
    };
    expect(turnError(v2Message)).toEqual(v2Expired);
    expect(sessionObservations(v2Message)).toMatchObject({ api_error: "auth_expiry", clears_api_error: false });

    const v1Expired = {
      name: "APIError",
      data: { statusCode: 401, isRetryable: false, message: "This API key has expired.", responseBody: expiredBody },
    };
    expect(classifyOpencodeConsoleKeyError(v1Expired, "opencode/deepseek-v4.1-flash-free")?.code).toBe("api_key_expired");
    expect(sessionObservations({ role: "assistant", providerID: "opencode", modelID: "beam-free", error: v1Expired })).toMatchObject({ api_error: "auth_expiry", clears_api_error: false });

    const invalidKey = {
      type: "provider.auth",
      status: 401,
      body: { error: { type: "authentication_error", code: "invalid_api_key", message: "Invalid API key" } },
    };
    expect(classifyOpencodeConsoleKeyError(invalidKey, "opencode/beam-free")).toMatchObject({
      code: "invalid_api_key",
      retry: true,
      commands: [
        { command: "opencode auth login" },
        { command: "opencode auth logout" },
      ],
    });
    expect(sessionObservations({ type: "assistant", model: { providerID: "opencode", id: "beam-free" }, error: invalidKey })).toMatchObject({ api_error: "authentication", clears_api_error: false });

    const genericFreeZen401 = { type: "provider.auth", status: 401, message: "Unauthorized" };
    expect(classifyOpencodeConsoleKeyError(genericFreeZen401, "opencode/beam-free")?.code).toBe("invalid_api_key");
  });
  it("keeps third-party providers, paid models without stale keys, and non-auth errors unaffected", () => {
    const expiredBody = JSON.stringify({ error: { type: "authentication_error", code: "api_key_expired", message: "This API key has expired." } });
    const thirdPartyExpired = { type: "provider.auth", status: 401, message: "This API key has expired.", response: { body: expiredBody } };
    expect(classifyOpencodeConsoleKeyError(thirdPartyExpired, "deepseek/deepseek-chat")).toBeNull();
    expect(classifyOpencodeConsoleKeyError(thirdPartyExpired, "anthropic/claude-sonnet-4-6")).toBeNull();
    expect(classifyOpencodeConsoleKeyError({ type: "provider.auth", status: 401, message: "API key required" }, "opencode/gpt-5.5")).toBeNull();
    expect(classifyOpencodeConsoleKeyError({ type: "provider.auth", status: 401, message: "Unauthorized" }, "opencode/gpt-5.5")).toBeNull();
    expect(classifyOpencodeConsoleKeyError({ type: "provider.quota", status: 402, body: { error: { type: "insufficient_balance_error" } } }, "opencode/beam-free")).toBeNull();
    expect(classifyOpencodeConsoleKeyError(error.error, "opencode/exo-free")).toBeNull();
    expect(sessionObservations({ type: "assistant", model: { providerID: "opencode", id: "beam-free" }, time: { completed: 50 } })).toEqual({ api_error: null, permission_mode: null, clears_api_error: true });
  });
});

import { DatabaseSync } from "node:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import ioSource_plugin from "../src/plugin";
import { withAsyncSqlite } from "./hostFixture.testdata";
const plugin = ioTestExport(ioSource_plugin);

// rusqlite binds ?N by index; node:sqlite 22.14 rejects ?N, so expand each to a positional "?".
function queryLikeHost(db: DatabaseSync, sql: string, params: Array<string | number>) {
  const ordered: Array<string | number> = [];
  const positional = sql.replace(/\?(\d+)/g, (_match, index: string) => {
    ordered.push(params[Number(index) - 1]);
    return "?";
  });
  return db.prepare(positional).all(...ordered);
}

it("bounds payload inspection to recent metadata candidates in mostly nonmatching history", () => {
  const db = new DatabaseSync(":memory:");
  try {
    db.exec("CREATE TABLE message(id TEXT PRIMARY KEY, time_updated INTEGER, data TEXT); CREATE INDEX recent_message ON message(time_updated DESC, id DESC);");
    const insert = db.prepare("INSERT INTO message VALUES (?, ?, ?)");
    const unavailable = (modelID: string) => JSON.stringify({ ...error, modelID });
    insert.run("outside", 0, unavailable("outside"));
    const user = JSON.stringify({ role: "user", content: "x".repeat(2048) });
    db.exec("BEGIN");
    for (let index = 1; index <= 2000; index++) insert.run(`user-${index}`, index, user);
    db.exec("COMMIT");
    insert.run("unavailable", 2001, unavailable("exo-free"));
    insert.run("recovered", 2002, JSON.stringify({ role: "assistant", providerID: "opencode", modelID: "recovered", time: { completed: 1 } }));
    insert.run("oversized", 2003, JSON.stringify({ role: "user", content: "x".repeat(16384) }));
    let payloadChecks = 0;
    db.function("json_valid", (_data: unknown) => { payloadChecks++; return 1; });
    const rows = queryLikeHost(db, MODEL_REJECTION_QUERY, [4096]) as Array<{ data: string }>;
    expect(payloadChecks).toBeGreaterThan(0);
    expect(payloadChecks).toBeLessThanOrEqual(200);
    const outcomes = rows.map(({ data }) => {
      const message: unknown = JSON.parse(data);
      return { model: sessionModelIdentity(message), message };
    });
    expect(outcomes.map(({ model }) => model)).toEqual(["opencode/recovered", "opencode/exo-free"]);
    expect([...rejectedModelIds(outcomes)]).toEqual(["opencode/exo-free"]);
  } finally {
    db.close();
  }
});

it("reads the 410 from a real SQLite own session, advances on message updates, and filters the failed model", () => {
  const dir = mkdtempSync(join(tmpdir(), "opencode-error-"));
  const db = new DatabaseSync(join(dir, "opencode.db"));
  const oldHost = Object.getOwnPropertyDescriptor(globalThis, "host");
  try {
    db.exec("CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT); CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);");
    db.prepare("INSERT INTO message VALUES (?, ?, ?, ?, ?)").run("msg_error", "ses_own", 10, 30, JSON.stringify(error));
    Object.defineProperty(globalThis, "host", { configurable: true, value: {
      platform: () => "linux",
      providerVersionJson: () => JSON.stringify({ version: "1.19.6", status: "detected" }),
      envGet: (key: string) => key === "OPENCODE_DB" ? join(dir, "opencode.db") : null,
      expandHome: (value: string) => value,
      exec: () => JSON.stringify({ code: 0, stdout: 'opencode/exo-free\n{"name":"Exo"}\nopencode/good\n{"name":"Good"}' }),
      execStart: () => JSON.stringify({ jobId: "discovery" }),
      awaitJob: (_id: string, then: (result: unknown) => unknown) => then({ code: 0, stdout: 'opencode/exo-free\n{"name":"Exo"}\nopencode/good\n{"name":"Good"}' }),
      unixNowMs: () => 1_000_000,
      fs: withAsyncSqlite({ fileExists: () => true, sqliteQuery: (_path: string, sql: string, params: Array<string | number>) => queryLikeHost(db, sql, params) }),
    } });
    const delta = plugin.readStructuredChat("/work", "ses_own", '[20,[]]') as {
      messages: Array<{ type: string; content: string }>;
      observations: Array<{ role: string; provider_session_id: string; provider_observations: { api_error: string } }>;
      cursor: string;
    };
    expect(delta.messages).toHaveLength(1);
    expect(delta.messages[0]).toMatchObject({ type: "system", systemSubtype: "api_error", content: "Model exo-free has been deprecated" });
    expect(delta.observations[0]).toMatchObject({ role: "assistant", provider_session_id: "ses_own", provider_observations: { api_error: "other" } });
    expect(plugin.readStructuredChat("/work", "ses_foreign", null)).toMatchObject({ messages: [], observations: [] });
    expect(plugin.readStructuredChat("/work", "ses_own", delta.cursor)).toMatchObject({ messages: [], observations: [] });
    expect(plugin.discoverModels()?.map((model) => model.id)).toEqual(["opencode/good"]);
    db.prepare("INSERT INTO message VALUES (?, ?, ?, ?, ?)").run("msg_success", "ses_own", 40, 40, JSON.stringify({ role: "assistant", providerID: "opencode", modelID: "good", time: { completed: 40 } }));
    expect(plugin.readStructuredChat("/work", "ses_own", delta.cursor)).toMatchObject({
      messages: [], observations: [{ role: "assistant", provider_session_id: "ses_own", provider_observations: { api_error: null, clears_api_error: true } }],
    });
    db.prepare("DELETE FROM message WHERE session_id = ?").run("ses_own");
    db.prepare("INSERT INTO message VALUES (?, ?, ?, ?, ?)").run("msg_foreign", "ses_foreign", 50, 50, JSON.stringify(error));
    expect(plugin.readStructuredChat("/work", "ses_own", null)).toMatchObject({ messages: [], observations: [] });
  } finally {
    if (oldHost) Object.defineProperty(globalThis, "host", oldHost);
    else Reflect.deleteProperty(globalThis, "host");
    db.close(); rmSync(dir, { recursive: true, force: true });
  }
});
