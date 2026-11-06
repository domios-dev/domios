import { timestampMs } from "./time";
import { parseDomiosMessage } from "./domios";
import { userEchoBody } from "./userEchoBody";
import type { NormalizedChatMessage } from "./types";

export function placeQueuedRows(rows: readonly NormalizedChatMessage[]): NormalizedChatMessage[] {
  const output: NormalizedChatMessage[] = [];
  for (const row of rows) {
    const notice = row.timelineMarker?.kind === "queued"
      && parseDomiosMessage(userEchoBody(row.content))?.attrs.from === "system";
    const at = notice ? timestampMs(row.timestamp) : null;
    const before = at == null ? -1 : output.findIndex(candidate => {
      if (candidate.sourceSessionId !== row.sourceSessionId
        || candidate.inheritedFromSessionId !== row.inheritedFromSessionId) return false;
      const candidateAt = timestampMs(candidate.timestamp);
      return candidateAt != null && candidateAt > at;
    });
    if (before < 0) output.push(row);
    else output.splice(before, 0, row);
  }
  return output;
}
