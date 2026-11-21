import { selectedDaemonTransport } from "./daemonRuntime";

export interface ResumableSessionItem {
  sessionId: string;
  provider: string;
  title: string;
  projectPath?: string;
  updatedAtSec?: number;
}

interface SessionSearchHitWire {
  meta?: {
    session_id?: string;
    provider?: string;
    title?: string | null;
    summary?: string | null;
    project_path?: string | null;
    modified_ts?: number | null;
    created_ts?: number | null;
  };
}

interface SessionSearchResponseWire {
  hits?: SessionSearchHitWire[];
}

export interface ResumableSessionsTransport {
  json<T>(method: "GET" | "POST", path: string, body?: unknown): Promise<T>;
}

export function formatSessionAge(updatedAtSec: number | undefined, nowMs = Date.now()): string | null {
  if (!updatedAtSec || !Number.isFinite(updatedAtSec)) return null;
  const deltaSec = Math.max(0, Math.floor(nowMs / 1000 - updatedAtSec));
  if (deltaSec < 60) return "just now";
  const minutes = Math.floor(deltaSec / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export async function listResumableSessions(
  cwd: string,
  limit = 5,
  transport: ResumableSessionsTransport = selectedDaemonTransport,
): Promise<ResumableSessionItem[]> {
  if (!cwd.trim()) return [];
  const query = new URLSearchParams({ q: "", project: cwd, limit: String(limit) });
  const response = await transport.json<SessionSearchResponseWire>("GET", `/api/sessions/search?${query.toString()}`);
  const items: ResumableSessionItem[] = [];
  for (const hit of response.hits ?? []) {
    const meta = hit.meta;
    if (!meta?.session_id || !meta.provider) continue;
    const title = (meta.title || meta.summary || meta.session_id).trim();
    const updatedAtSec = meta.modified_ts ?? meta.created_ts ?? undefined;
    items.push({
      sessionId: meta.session_id,
      provider: meta.provider,
      title,
      ...(meta.project_path ? { projectPath: meta.project_path } : {}),
      ...(updatedAtSec ? { updatedAtSec } : {}),
    });
  }
  return items;
}

export async function resumeListedSession(
  sessionId: string,
  workspaceId: string,
  transport: ResumableSessionsTransport = selectedDaemonTransport,
): Promise<{ tabId?: string; workspaceId?: string }> {
  const response = await transport.json<{ tab_id?: string; workspace_id?: string }>(
    "POST",
    `/api/sessions/${encodeURIComponent(sessionId)}/resume`,
    { workspace_id: workspaceId },
  );
  return { tabId: response.tab_id, workspaceId: response.workspace_id };
}
