import { execFile } from "node:child_process";
import { promisify } from "node:util";
import path from "node:path";
import { homedir } from "node:os";
import { readFileSync, statSync } from "node:fs";
import type { ApiClient } from "./api-client";
import { expect } from "./fixtures";
import { repoRoot } from "./dev-instance";

const execute = promisify(execFile);

export async function deliveryLog(tabId: string) {
  const logPath = process.env.PW_SWITCH_DEV_LOG ?? path.join(homedir(), ".codeterm-dev", "logs", "codeterm.log");
  const result = await execute("rg", ["--fixed-strings", tabId, logPath], { timeout: 15_000, maxBuffer: 8 * 1024 * 1024 });
  return { logPath, tabId, lines: result.stdout.split(/\r?\n/).filter(line => /chat-send|delivery-timeline|\[agent-model\/timing\]/.test(line)) };
}
export type Model = { id: string; display_name: string; primary?: boolean; reasoning_efforts?: { id: string }[] };
export type Descriptor = { id: string; default_model: string; model_switch: string; available_models: Model[]; reasoning_efforts: { id: string }[]; supports_permission_bypass?: boolean };
export type Identity = { agent_session_id: string; session_binding_nonce: string; pty_id: string; agent_session_model: string; reasoning_effort_identity?: { actual?: string } };
export type RawRecord = {
  id?: string; type?: string; role?: string; parentID?: string; modelID?: string; providerID?: string; variant?: string;
  time?: { completed?: number }; parts?: { type?: string; text?: string }[];
  message?: { content?: unknown; model?: string; effort?: string; stop_reason?: string }; effort?: string;
  payload?: { role?: string; content?: unknown; turn_id?: string; model?: string; effort?: string; type?: string; last_agent_message?: string };
  metadata?: { retained_source?: { id?: { turn_id?: string } } };
  params?: { sessionId?: string; update?: { sessionUpdate?: string; content?: unknown; prompt_id?: string;
    _meta?: { modelId?: string; reasoningEffort?: string }; usage?: { modelUsage?: Record<string, unknown> } };
    _meta?: { promptId?: string } };
};
export type Evidence = { path: string; session: string; records: RawRecord[] };
export type RestoreTab = { provider: string; tabId: string; session: string; nonce: string; pty: string; a: string; b: string };
export type RestoreManifest = { workspace: string; tabs: RestoreTab[] };

export function switchModels(descriptor: Descriptor): { a: Model; b: Model } {
  const explicitEfforts = (model: Model) => (model.reasoning_efforts ?? descriptor.reasoning_efforts).filter(e => e.id !== "default");
  const effortModels = descriptor.available_models.filter(model => explicitEfforts(model).length >= 2);
  const choices = effortModels.length >= 2 ? effortModels : descriptor.available_models;
  const a = choices.find(model => model.id === descriptor.default_model) ?? choices[0];
  const b = choices.find(model => model.id !== a?.id && (model.primary || !choices.some(c => c.primary)));
  if (!a || !b) throw new Error("Two switchable catalogue models required");
  return { a, b };
}

export async function grokEffortEvidence(session: string, switchedAtMs: number, effort: string) {
  const meta = await dc<{ file_path: string }>("sessions", "get", session);
  const summaryPath = path.join(path.dirname(meta.file_path), "summary.json");
  const summary = JSON.parse(readFileSync(summaryPath, "utf8")) as { info?: { id?: string }; reasoning_effort?: string };
  const updatedAtMs = statSync(summaryPath).mtimeMs;
  const observedAtMs = Date.now();
  expect(summary.info?.id, "Grok effort belongs to the exact bound session").toBe(session);
  expect(summary.reasoning_effort, "provider-authored session effort").toBe(effort);
  expect(updatedAtMs, "summary must be updated after Apply").toBeGreaterThan(switchedAtMs);
  expect(updatedAtMs).toBeLessThanOrEqual(observedAtMs);
  return { kind: "session-level evidence", summaryPath, updatedAtMs, observedAtMs, switchedAtMs, summary };
}

export async function dc<T>(...args: string[]): Promise<T> {
  const shell = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
  const result = await execute(shell, [path.join(__dirname, "live-switch-dc.sh"), ...args, "--json"],
    { cwd: repoRoot(), timeout: 180_000, maxBuffer: 8 * 1024 * 1024 });
  return JSON.parse(result.stdout);
}

export async function identity(api: ApiClient, tabId: string): Promise<Identity> {
  const tabs = await api.get<Array<Identity & { id: string }>>(`/tabs/inspect?tab_ids=${encodeURIComponent(tabId)}`);
  expect(tabs, "identity inspection returns exactly the requested scratch tab").toHaveLength(1);
  const tab = tabs[0];
  expect(tab, `scratch tab ${tabId} still exists`).toBeDefined();
  expect(tab.id).toBe(tabId);
  return tab;
}

export async function records(provider: string, session: string, knownPath?: string): Promise<Evidence> {
  const meta = knownPath ? undefined : await dc<{ transcript_path?: string; transcriptPath?: string; file_path?: string; filePath?: string }>("sessions", "get", session);
  const file = knownPath ?? meta?.transcript_path ?? meta?.transcriptPath ?? meta?.file_path ?? meta?.filePath;
  const result = await execute("python", [path.join(__dirname, "live-switch-records.py"), provider, session, file ?? ""],
    { timeout: 15_000, maxBuffer: 16 * 1024 * 1024 });
  return JSON.parse(result.stdout);
}

export function turnEvidence(provider: string, evidence: Evidence, marker: string): { model: string; effort?: string; raw: RawRecord[] } {
  const rows = evidence.records;
  if (provider === "claude") {
    const start = rows.findIndex(row => row.type === "user" && JSON.stringify(row.message?.content).includes(marker));
    if (start < 0) throw new Error(`No exact user record ${marker} in ${evidence.path}`);
    const end = rows.findIndex((row, i) => i > start && row.type === "user");
    const turn = rows.slice(start, end < 0 ? undefined : end);
    const answer = turn.find(row => row.type === "assistant" && row.message?.model);
    if (!answer?.message?.model) throw new Error(`No assistant model record after ${marker}`);
    return { model: answer.message.model, effort: answer.message.effort ?? answer.effort, raw: turn };
  }
  if (provider === "codex") {
    const user = rows.find(row => row.type === "response_item" && row.payload?.role === "user" && JSON.stringify(row.payload.content).includes(marker));
    const turnId = user?.metadata?.retained_source?.id?.turn_id;
    if (!turnId) throw new Error(`No exact Codex retained user turn ${marker}`);
    const turn = rows.filter(row => row.payload?.turn_id === turnId || row.metadata?.retained_source?.id?.turn_id === turnId);
    const context = turn.find(row => row.type === "turn_context");
    if (!context?.payload?.model) throw new Error(`No Codex turn_context after ${marker}`);
    return { model: context.payload.model, effort: context.payload.effort, raw: turn };
  }
  if (provider === "grok") {
    const start = rows.findIndex(row => row.params?.update?.sessionUpdate === "user_message_chunk" && JSON.stringify(row).includes(marker));
    if (start < 0) throw new Error(`No exact Grok user record ${marker}`);
    const end = rows.findIndex((row, i) => i > start && row.params?.update?.sessionUpdate === "user_message_chunk");
    const turn = rows.slice(start, end < 0 ? undefined : end);
    const user = rows[start].params!.update!;
    const answer = turn.find(row => row.params?.update?.sessionUpdate === "agent_message_chunk");
    const completed = turn.find(row => row.params?.update?.sessionUpdate === "turn_completed" && row.params.update.prompt_id === answer?.params?._meta?.promptId);
    const actualModels = Object.keys(completed?.params?.update?.usage?.modelUsage ?? {});
    if (!user._meta?.modelId || !answer || rows[start].params?.sessionId !== evidence.session || !actualModels.length ||
        actualModels.some(model => model !== user._meta!.modelId && model !== `${user._meta!.modelId}-build`)) {
      throw new Error(`No Grok turn model and assistant append after ${marker}`);
    }
    return { model: user._meta.modelId, effort: user._meta.reasoningEffort, raw: turn };
  }
  const user = rows.find(row => row.role === "user" && JSON.stringify(row).includes(marker));
  const answer = rows.find(row => row.role === "assistant" && row.parentID === user?.id && row.time?.completed &&
    row.parts?.some(part => part.type === "text" && part.text?.includes(marker)));
  if (!answer) throw new Error(`OpenCode exact parent-linked assistant turn missing for ${marker}`);
  return { model: `${answer.providerID}/${answer.modelID}`, effort: user?.variant, raw: [user!, answer] };
}

export function assertModel(provider: string, actual: string, target: string): void {
  if (provider === "claude" && !target.startsWith("claude-")) {
    expect(actual, `resolved Claude alias ${target} in exact assistant record`).toMatch(new RegExp(`^claude-${target}-`));
  } else expect(actual, "exact turn-record model").toBe(target);
}

export function restoreManifest(): RestoreManifest | undefined {
  const file = process.env.PW_SWITCH_RESTORE;
  return file ? JSON.parse(readFileSync(file, "utf8")) : undefined;
}
