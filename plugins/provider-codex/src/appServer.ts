// Codex remains the trust-hash authority because its private hash algorithm can change across releases.
import { CODETERM_CODEX_HOOK_MARKER } from "./hooks";

declare const host: {
  platform(): string;
  exec: ((optsJson: string) => string) & {
    async?: (
      opts: {
        bin: string;
        args: string[];
        conversation: { stdin: string; awaitResponseIds: number[] }[];
        timeoutMs: number;
      },
      then: (result: unknown) => unknown,
    ) => unknown;
  };
};

export interface CodexTrustGrantOutcome {
  granted: boolean;
  /** True when config/batchWrite confirmed an applied edit (false: already
   *  trusted, or the write never confirmed). */
  wrote: boolean;
  /** True when the app-server RPC surface is unavailable (old CLI) — the
   *  enable-only install behavior stands and hook_trust_state stays honest. */
  degraded: boolean;
  /** True when the post-write hooks/list re-verify failed closed. */
  verify_failed: boolean;
  reason?: string;
  entries?: { key: string; hash: string }[];
}

interface HookListing {
  key: string;
  command: string | null;
  currentHash: string;
  trustStatus: string;
}

interface RpcRequest {
  method: string;
  id?: number;
  params?: unknown;
}

interface RpcError {
  code?: number;
  message?: string;
}

type SessionOutcome =
  | { kind: "ok"; responses: Record<number, { result?: unknown; error?: RpcError }> }
  | { kind: "unavailable"; reason: string }
  | { kind: "failed"; reason: string };

export interface CodexHookTrustInspection {
  verdict: "trusted" | "stale" | "unknown";
  reason?: string;
}

// Bounded failure deadline around process exit (the server exits on stdin
// EOF); elapsed time is never treated as readiness or success.
const APP_SERVER_EXIT_DEADLINE_MS = 20000;
const JSON_RPC_METHOD_NOT_FOUND = -32601;

function stderrIndicatesMissingAppServer(stderr: string): boolean {
  return /unrecognized subcommand|unknown subcommand|invalid subcommand|unexpected argument/i.test(stderr);
}

function isMethodNotFound(error: RpcError): boolean {
  return error.code === JSON_RPC_METHOD_NOT_FOUND || /method not found/i.test(String(error.message || ""));
}

/** One short-lived `codex app-server` stdio JSONL session. Each stage waits for
 *  its response before stdin advances; EOF closes the server after the RPCs. */
function parseAppServerSession(raw: unknown): SessionOutcome {
  let result: { code?: number; stdout?: string; stderr?: string; error?: string };
  try {
    result = typeof raw === "string" ? JSON.parse(raw) : raw as typeof result;
  } catch {
    return { kind: "failed", reason: "host.exec returned invalid JSON" };
  }
  if (!result || typeof result !== "object") {
    return { kind: "failed", reason: "host.exec returned invalid JSON" };
  }
  const stderr = String(result.stderr || "");
  if (typeof result.error === "string" && result.error) {
    if (result.error.indexOf("spawn ") === 0) {
      return { kind: "unavailable", reason: "codex binary unavailable: " + result.error };
    }
    return { kind: "failed", reason: result.error };
  }
  const responses: Record<number, { result?: unknown; error?: RpcError }> = {};
  const outLines = String(result.stdout || "").split("\n");
  for (let i = 0; i < outLines.length; i++) {
    const line = outLines[i].trim();
    if (!line) continue;
    let message: { id?: number; result?: unknown; error?: RpcError };
    try { message = JSON.parse(line); } catch { continue; }
    if (typeof message.id === "number") responses[message.id] = { result: message.result, error: message.error };
  }
  const init = responses[1];
  if (!init) {
    if (stderrIndicatesMissingAppServer(stderr)) {
      return { kind: "unavailable", reason: "codex CLI does not support app-server: " + stderr.slice(0, 400) };
    }
    return { kind: "failed", reason: "codex app-server exited before responding" + (stderr ? ": " + stderr.slice(0, 400) : "") };
  }
  if (init.error) {
    if (isMethodNotFound(init.error)) return { kind: "unavailable", reason: "codex app-server does not support initialize" };
    return { kind: "failed", reason: "initialize failed: " + String(init.error.message || "unknown error") };
  }
  if (typeof result.code === "number" && result.code !== 0) {
    return { kind: "failed", reason: "codex app-server exited with code " + result.code };
  }
  return { kind: "ok", responses };
}

function runAppServerSession(requests: RpcRequest[], then: (session: SessionOutcome) => unknown): unknown {
  const initialize = JSON.stringify({
      method: "initialize",
      id: 1,
      params: {
        clientInfo: { name: "codeterm", title: "CodeTerm", version: "0.0.0" },
        capabilities: { experimentalApi: true },
      },
    });
  const requestLines: string[] = [JSON.stringify({ method: "initialized" })];
  const responseIds: number[] = [];
  for (let i = 0; i < requests.length; i++) {
    const req: Record<string, unknown> = { method: requests[i].method, id: requests[i].id };
    if (requests[i].params !== undefined) req.params = requests[i].params;
    requestLines.push(JSON.stringify(req));
    const requestId = requests[i].id;
    if (typeof requestId === "number") responseIds.push(requestId);
  }
  const opts = {
    bin: "codex",
    args: ["app-server"],
    conversation: [
      { stdin: initialize + "\n", awaitResponseIds: [1] },
      { stdin: requestLines.join("\n") + "\n", awaitResponseIds: responseIds },
    ],
    timeoutMs: APP_SERVER_EXIT_DEADLINE_MS,
  };
  try {
    const asyncExec = host.exec.async;
    return typeof asyncExec === "function"
      ? asyncExec(opts, (raw) => then(parseAppServerSession(raw)))
      : then(parseAppServerSession(host.exec(JSON.stringify(opts))));
  } catch (e) {
    return then({ kind: "failed", reason: "host.exec threw: " + String(e) });
  }
}

function requestOutcome(
  session: SessionOutcome,
  id: number,
  method: string,
): { kind: "ok"; result: unknown } | { kind: "unavailable"; reason: string } | { kind: "failed"; reason: string } {
  if (session.kind !== "ok") return session;
  const response = session.responses[id];
  if (!response) return { kind: "failed", reason: "codex app-server returned no response for " + method };
  if (response.error) {
    if (isMethodNotFound(response.error)) {
      return { kind: "unavailable", reason: "codex app-server does not support " + method };
    }
    return { kind: "failed", reason: method + " failed: " + String(response.error.message || "unknown error") };
  }
  return { kind: "ok", result: response.result };
}

function collectHookListings(result: unknown): HookListing[] {
  const data = result && typeof result === "object" && Array.isArray((result as { data?: unknown }).data)
    ? ((result as { data: unknown[] }).data as { hooks?: unknown }[])
    : [];
  const listings: HookListing[] = [];
  const seen: Record<string, boolean> = {};
  for (let i = 0; i < data.length; i++) {
    const hooks = data[i] && Array.isArray(data[i].hooks) ? (data[i].hooks as Record<string, unknown>[]) : [];
    for (let j = 0; j < hooks.length; j++) {
      const hook = hooks[j];
      if (!hook || typeof hook.key !== "string" || typeof hook.currentHash !== "string" || typeof hook.trustStatus !== "string") {
        continue;
      }
      // hooks/list repeats user-scope hooks per requested cwd; count each key once.
      if (seen[hook.key]) continue;
      seen[hook.key] = true;
      listings.push({
        key: hook.key,
        command: typeof hook.command === "string" ? hook.command : null,
        currentHash: hook.currentHash,
        trustStatus: hook.trustStatus,
      });
    }
  }
  return listings;
}

function slashNormalize(value: string): string {
  return value.split("\\").join("/");
}

function keyMatchesManagedLabel(key: string, hooksPath: string, label: string): boolean {
  return slashNormalize(key).indexOf(slashNormalize(hooksPath) + ":" + label + ":") === 0;
}

function isCodetermManagedHookCommand(command: string | null): boolean {
  const c = String(command || "");
  return c.indexOf("/hook/session_start") !== -1
    || c.indexOf("/hook/stop") !== -1
    || c.indexOf("codeterm-session-start.ps1") !== -1
    || (c.indexOf("codeterm-session-hook.cmd") !== -1
      && c.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1);
}

function managedListings(listings: HookListing[], hooksPath: string, labels: string[]): HookListing[] {
  const out: HookListing[] = [];
  for (let i = 0; i < listings.length; i++) {
    const listing = listings[i];
    if (!isCodetermManagedHookCommand(listing.command)) continue;
    for (let j = 0; j < labels.length; j++) {
      if (keyMatchesManagedLabel(listing.key, hooksPath, labels[j])) {
        out.push(listing);
        break;
      }
    }
  }
  return out;
}

function trustTableKey(line: string): string | null {
  const value = line.trim();
  const prefix = "[hooks.state.";
  if (!value.startsWith(prefix) || !value.endsWith("]")) return null;
  const raw = value.slice(prefix.length, -1);
  if (raw.length >= 2 && raw[0] === "'" && raw[raw.length - 1] === "'") {
    return raw.slice(1, -1);
  }
  if (raw.startsWith('"') && raw.endsWith('"')) {
    try { return JSON.parse(raw); } catch (_) { return null; }
  }
  return null;
}

function recordedTrustedHash(config: string, key: string): string | null {
  const lines = config.split("\n");
  const normalizedKey = slashNormalize(key);
  for (let i = 0; i < lines.length; i++) {
    const tableKey = trustTableKey(lines[i]);
    const normalizedTableKey = tableKey === null ? null : slashNormalize(tableKey);
    const sameKey = host.platform() === "windows"
      ? normalizedTableKey?.toLowerCase() === normalizedKey.toLowerCase()
      : normalizedTableKey === normalizedKey;
    if (!sameKey) continue;
    for (let j = i + 1; j < lines.length; j++) {
      const trimmed = lines[j].trim();
      if (trimmed.startsWith("[") && trimmed.endsWith("]")) break;
      const match = /^trusted_hash\s*=\s*(['"])([^'"\r\n]+)\1\s*(?:#.*)?\r?$/.exec(lines[j]);
      if (match) return match[2];
    }
    return null;
  }
  return null;
}

export function inspectCodexHookTrust(opts: {
  hooksPath: string;
  config: string;
  managedLabels: string[];
  listCwd: string;
}, then: (inspection: CodexHookTrustInspection) => unknown = (inspection) => inspection): unknown {
  return runAppServerSession([
    { method: "hooks/list", id: 2, params: { cwds: [opts.listCwd] } },
  ], (session) => {
    const result = requestOutcome(session, 2, "hooks/list");
    if (result.kind !== "ok") return then({ verdict: "unknown", reason: result.reason });
    const managed = managedListings(collectHookListings(result.result), opts.hooksPath, opts.managedLabels);
    if (managed.length !== opts.managedLabels.length) {
      return then({
        verdict: "unknown",
        reason: "Codex returned " + managed.length + " managed hooks, expected " + opts.managedLabels.length,
      });
    }
    for (let i = 0; i < managed.length; i++) {
      const listing = managed[i];
      const recorded = recordedTrustedHash(opts.config, listing.key);
      if (recorded === null) {
        return then({ verdict: "unknown", reason: "No recorded trusted_hash exists for " + listing.key });
      }
      if (recorded !== listing.currentHash) {
        return then({ verdict: "stale", reason: "Recorded trusted_hash does not match Codex currentHash for " + listing.key });
      }
      if (listing.trustStatus !== "trusted") {
        return then({ verdict: "unknown", reason: "Codex reports trustStatus=" + listing.trustStatus + " for " + listing.key });
      }
    }
    return then({ verdict: "trusted" });
  });
}

function outcome(partial: Partial<CodexTrustGrantOutcome>): CodexTrustGrantOutcome {
  return {
    granted: false,
    wrote: false,
    degraded: false,
    verify_failed: false,
    ...partial,
  };
}

/**
 * Grants Codex hook trust for exactly the managed CodeTerm hook entries via
 * one bounded RPC lane: hooks/list → config/batchWrite → hooks/list re-verify.
 * The upserted hash is always the hash Codex itself reported on hooks/list.
 * Failure paths never write config directly: Codex owns every trust-hash edit.
 */
export function grantCodexHookTrust(opts: {
  hooksPath: string;
  configPath: string | null;
  managedLabels: string[];
  listCwd: string;
}, then: (result: CodexTrustGrantOutcome) => unknown = (result) => result): unknown {
  if (!opts.configPath) {
    return then(outcome({ degraded: true, reason: "could not resolve Codex config path" }));
  }
  return grantCodexHookTrustFromCodex(opts, then);
}

function grantCodexHookTrustFromCodex(opts: {
  hooksPath: string;
  configPath: string | null;
  managedLabels: string[];
  listCwd: string;
}, then: (result: CodexTrustGrantOutcome) => unknown): unknown {
  if (!opts.managedLabels.length) {
    return then(outcome({ degraded: true, reason: "no managed hook keys declared" }));
  }
  if (!opts.configPath) {
    return then(outcome({ degraded: true, reason: "could not resolve Codex config path" }));
  }

  return runAppServerSession([
    { method: "hooks/list", id: 2, params: { cwds: [opts.listCwd] } },
  ], (listSession) => {
    const listResult = requestOutcome(listSession, 2, "hooks/list");
    if (listResult.kind === "unavailable") return then(outcome({ degraded: true, reason: listResult.reason }));
    if (listResult.kind === "failed") return then(outcome({ reason: listResult.reason }));

    const managed = managedListings(collectHookListings(listResult.result), opts.hooksPath, opts.managedLabels);
    if (managed.length !== opts.managedLabels.length) {
      return then(outcome({
        verify_failed: true,
        reason: "hooks/list reported " + managed.length + " managed entries, expected " + opts.managedLabels.length,
      }));
    }

    const needingTrust: HookListing[] = [];
    for (let i = 0; i < managed.length; i++) {
      if (managed[i].trustStatus !== "trusted") needingTrust.push(managed[i]);
    }
    if (!needingTrust.length) {
      return then(outcome({
        granted: true,
        entries: managed.map((listing) => ({ key: listing.key, hash: listing.currentHash })),
      }));
    }

    if (!opts.configPath) return then(outcome({ reason: "Codex config path is unknown; trust was not granted" }));
    const value: Record<string, { trusted_hash: string }> = {};
    for (let i = 0; i < needingTrust.length; i++) {
      const currentHash = needingTrust[i].currentHash;
      if (!currentHash) return then(outcome({ reason: "Codex reported no current hash for a hook that needs trust" }));
      value[needingTrust[i].key] = { trusted_hash: currentHash };
    }
    return runAppServerSession([
      {
        method: "config/batchWrite",
        id: 2,
        params: { edits: [{ keyPath: "hooks.state", value, mergeStrategy: "upsert" }], reloadUserConfig: true },
      },
      { method: "hooks/list", id: 3, params: { cwds: [opts.listCwd] } },
    ], (writeSession) => {
      const writeResult = requestOutcome(writeSession, 2, "config/batchWrite");
      if (writeResult.kind !== "ok") return then(outcome({ reason: writeResult.reason }));
      const writeStatus = writeResult.result && typeof writeResult.result === "object"
        ? (writeResult.result as { status?: unknown }).status
        : undefined;
      if (writeStatus !== "ok") {
        return then(outcome({ reason: "config/batchWrite did not confirm status=ok" }));
      }
      const verifyResult = requestOutcome(writeSession, 3, "hooks/list");
      if (verifyResult.kind !== "ok") {
        return then(outcome({ wrote: true, verify_failed: true, reason: verifyResult.reason }));
      }

      const verified = managedListings(collectHookListings(verifyResult.result), opts.hooksPath, opts.managedLabels);
      if (verified.length !== opts.managedLabels.length) {
        return then(outcome({ wrote: true, verify_failed: true, reason: "post-grant verify reported " + verified.length + " managed entries" }));
      }
      const expectedHashes: Record<string, string> = {};
      for (let i = 0; i < managed.length; i++) expectedHashes[managed[i].key] = managed[i].currentHash;
      for (let i = 0; i < verified.length; i++) {
        const listing = verified[i];
        if (listing.trustStatus !== "trusted") {
          return then(outcome({ wrote: true, verify_failed: true, reason: "post-grant verify left " + listing.key + " " + listing.trustStatus }));
        }
        if (expectedHashes[listing.key] !== listing.currentHash) {
          return then(outcome({ wrote: true, verify_failed: true, reason: "post-grant verify reported a different hash for " + listing.key }));
        }
      }
      return then(outcome({
        granted: true,
        wrote: true,
        entries: verified.map((listing) => ({ key: listing.key, hash: listing.currentHash })),
      }));
    });
  });
}
