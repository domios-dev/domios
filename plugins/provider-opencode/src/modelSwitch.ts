import type { SessionModelTranscriptBoundary } from "@codeterm/plugin-sdk";
import { ASSISTANT_MESSAGE_MAX_BYTES, opencodeDbPath, safeJson } from "./helpers";

export const modelSwitch = {
  sessionModelTranscriptBoundary(_cwd: string, sessionId: string): SessionModelTranscriptBoundary | null {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery<{ id: string; time_created: number }>(db,
      "SELECT id, time_created FROM message WHERE session_id = ?1 ORDER BY time_created DESC, id DESC LIMIT 1",
      [sessionId]);
    const last = rows[0];
    return { lastEntryUuid: last?.id ?? null, lastEntryTimestamp: String(last?.time_created ?? 0), lastEntryType: null, boundaryFound: true };
  },

  detectSessionModelAfterBoundary(_cwd: string, sessionId: string, boundary: SessionModelTranscriptBoundary): string | null {
    const db = opencodeDbPath();
    const timestamp = Number(boundary.lastEntryTimestamp);
    if (!boundary.boundaryFound || !Number.isFinite(timestamp) || !db || !host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery<{ data: string }>(db,
      "SELECT data FROM message WHERE session_id = ?1 AND (time_created > ?2 OR (time_created = ?2 AND id > ?3)) " +
      "AND octet_length(data) <= ?4 ORDER BY time_created ASC, id ASC LIMIT 128",
      [sessionId, timestamp, boundary.lastEntryUuid ?? "", ASSISTANT_MESSAGE_MAX_BYTES]);
    for (const row of rows) {
      const message = safeJson(row.data) as { role?: string; providerID?: string; modelID?: string; parentID?: string } | null;
      if (message?.role === "assistant" && message.providerID && message.modelID && message.parentID) {
        return `${message.providerID}/${message.modelID}`;
      }
    }
    return null;
  },
};
