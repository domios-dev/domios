import { invoke } from "./bridge";
import type { AtomicSpawn } from "./types";

export type SessionLaunchIntent = "fresh_general";

export interface FreshGeneralSessionRequest {
  workspaceId: string;
  providerId: string;
  cwd: string;
  role: "general_agent";
  intent: SessionLaunchIntent;
}

export function freshGeneralSessionRequest(
  workspaceId: string,
  providerId: string,
  cwd: string | undefined,
): FreshGeneralSessionRequest {
  if (!cwd || !providerId) throw new Error("General session configuration is unavailable");
  return { workspaceId, providerId, cwd, role: "general_agent", intent: "fresh_general" };
}

export function spawnFreshGeneralSession(
  workspaceId: string,
  providerId: string,
  cwd: string | undefined,
  request: typeof invoke = invoke,
): Promise<AtomicSpawn> {
  return request<AtomicSpawn>("spawn_general_agent_session", {
    params: freshGeneralSessionRequest(workspaceId, providerId, cwd),
  });
}
