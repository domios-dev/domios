import { describe, expect, it } from "vitest";
import { parseOpencodeModels } from "../src/helpers";
import { rejectedModelIds, sessionObservations, turnError, unavailableModelError } from "../src/turnErrors";

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
  it("uses the most recent completed outcome for each model", () => {
    const success = { role: "assistant", time: { completed: 50 } };
    expect([...rejectedModelIds([{ model: "opencode/exo-free", message: error }, { model: "opencode/good", message: success }])]).toEqual(["opencode/exo-free"]);
    expect([...rejectedModelIds([{ model: "opencode/exo-free", message: success }, { model: "opencode/exo-free", message: error }])]).toEqual([]);
  });
  it("filters live deprecated and unavailable models while retaining unknown availability", () => {
    expect(parseOpencodeModels('opencode/exo-free\n{"status":"deprecated"}\nvendor/off\n{"available":false}\nvendor/unknown\n{"name":"Unknown"}')?.map((model) => model.id)).toEqual(["vendor/unknown"]);
    expect(parseOpencodeModels('opencode/exo-free\n{"status":"deprecated"}')).toEqual([]);
  });
});

import { DatabaseSync } from "node:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import plugin from "../src/plugin";

// rusqlite binds ?N by index; node:sqlite 22.14 rejects ?N, so expand each to a positional "?".
function queryLikeHost(db: DatabaseSync, sql: string, params: Array<string | number>) {
  const ordered: Array<string | number> = [];
  const positional = sql.replace(/\?(\d+)/g, (_match, index: string) => {
    ordered.push(params[Number(index) - 1]);
    return "?";
  });
  return db.prepare(positional).all(...ordered);
}

it("reads the 410 from a real SQLite own session, advances on message updates, and filters the failed model", () => {
  const dir = mkdtempSync(join(tmpdir(), "opencode-error-"));
  const db = new DatabaseSync(join(dir, "opencode.db"));
  const oldHost = Object.getOwnPropertyDescriptor(globalThis, "host");
  try {
    db.exec("CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT); CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);");
    db.prepare("INSERT INTO message VALUES (?, ?, ?, ?, ?)").run("msg_error", "ses_own", 10, 30, JSON.stringify(error));
    Object.defineProperty(globalThis, "host", { configurable: true, value: {
      platform: () => "linux",
      envGet: (key: string) => key === "OPENCODE_DB" ? join(dir, "opencode.db") : null,
      expandHome: (value: string) => value,
      exec: () => JSON.stringify({ code: 0, stdout: 'opencode/exo-free\n{"name":"Exo"}\nopencode/good\n{"name":"Good"}' }),
      fs: { fileExists: () => true, sqliteQuery: (_path: string, sql: string, params: Array<string | number>) => queryLikeHost(db, sql, params) },
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
