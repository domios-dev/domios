import { intentGesture, viewHostResult, type ViewHostGate, type ViewHostIntent, type ViewHostResult } from "./pluginViewHostActions";
import type { OpenJobDetail } from "../components/Jobs/openJobEvent";

export interface PluginRunNavigation {
  resolve(reference: string): Promise<unknown>;
  open(target: OpenJobDetail): void;
}

function target(value: unknown): OpenJobDetail | null {
  if (!value || typeof value !== "object") return null;
  const result = value as Record<string, unknown>;
  const raw = result.target;
  if (!raw || typeof raw !== "object") return null;
  const run = raw as Record<string, unknown>;
  const id = (value: unknown): value is string => typeof value === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(value);
  if (!id(result.workspaceId) || !id(run.blueprintId) || !id(run.runId) || !(run.teamId === null || id(run.teamId))) return null;
  return {workspaceId:result.workspaceId,target:{blueprintId:run.blueprintId,runId:run.runId,teamId:run.teamId}};
}

export async function openPluginRun(reference: unknown, intent: ViewHostIntent, isCurrent: () => boolean,
  gate: ViewHostGate, deps: PluginRunNavigation): Promise<ViewHostResult> {
  if (typeof reference !== "string" || !/^[a-f0-9]{64}$/.test(reference)) return {ok:false,code:"invalid",reason:"reference"};
  if (!intentGesture(intent)) return {ok:false,code:"denied",reason:"gesture"};
  if (!isCurrent()) return {ok:false,code:"denied",reason:"stale"};
  if (!gate.claim()) return {ok:false,code:"denied",reason:"busy"};
  let acted = false;
  try {
    const answer = await deps.resolve(reference);
    if (!isCurrent()) return {ok:false,code:"denied",reason:"stale"};
    if (answer && typeof answer === "object" && (answer as {ok?:unknown}).ok === false) {
      const refusal = viewHostResult(answer);
      if (!refusal.ok) return refusal;
    }
    const resolved = target(answer);
    if (!resolved) return {ok:false,code:"unavailable",reason:"run"};
    acted = true;
    deps.open(resolved);
    return {ok:true};
  } catch { return isCurrent() ? {ok:false,code:"unavailable",reason:"run"} : {ok:false,code:"denied",reason:"stale"}; }
  finally { gate.release(acted); }
}
