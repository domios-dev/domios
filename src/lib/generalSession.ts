import { invoke } from "./bridge";
import type { AtomicSpawn } from "./types";

export type SessionLaunchIntent = "fresh_general";

export interface FreshGeneralSessionRequest {
  workspaceId?: string;
  providerId?: string;
  cwd?: string;
  role: "general_agent";
  intent: SessionLaunchIntent;
  model?: string;
  reasoningEffort?: string;
  preset?: string;
  accountId?: string;
}

type FreshGeneralSelection = Pick<FreshGeneralSessionRequest, "model" | "reasoningEffort" | "preset" | "accountId">;

export function freshGeneralSessionRequest(
  workspaceId: string,
  providerId: string,
  cwd: string | undefined,
  selection: FreshGeneralSelection = {},
): FreshGeneralSessionRequest {
  if (!cwd || !providerId) throw new Error("General session configuration is unavailable");
  return { ...selection, workspaceId, providerId, cwd, role: "general_agent", intent: "fresh_general" };
}

export function freshGeneralPeerSessionRequest() {
  return { role: "general_agent" as const, intent: "fresh_general" as const };
}

export function spawnFreshGeneralSession(
  workspaceId: string,
  providerId: string,
  cwd: string | undefined,
  request: typeof invoke = invoke,
  selection: FreshGeneralSelection = {},
  peerId?: string,
): Promise<AtomicSpawn> {
  if (peerId !== undefined) return request<AtomicSpawn>("spawn_general_agent_session", { peerId, params: freshGeneralPeerSessionRequest() });
  return request<AtomicSpawn>("spawn_general_agent_session", {
    params: freshGeneralSessionRequest(workspaceId, providerId, cwd, selection),
  });
}
