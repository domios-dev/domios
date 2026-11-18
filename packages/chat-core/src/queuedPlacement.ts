import { timestampMs } from "./time";
import { parseDomiosMessage } from "./domios";
import { userEchoBody } from "./userEchoBody";
import type { NormalizedChatMessage } from "./types";

export function placeQueuedRows(
  rows: readonly NormalizedChatMessage[],
  isStructuredInput: (row: NormalizedChatMessage) => boolean,
): NormalizedChatMessage[] {
  const output: NormalizedChatMessage[] = [];
  const times = new Map<NormalizedChatMessage, number>();
  let latestAt = -Infinity;
  for (const row of rows) {
    const notice = row.timelineMarker?.kind === "queued"
      && parseDomiosMessage(userEchoBody(row.content))?.attrs.from === "system";
    const at = timestampMs(row.timestamp);
    const before = at == null || at >= latestAt || !(notice || isStructuredInput(row)) ? -1 : output.findIndex(candidate => {
      if (candidate.sourceSessionId !== row.sourceSessionId
        || candidate.inheritedFromSessionId !== row.inheritedFromSessionId) return false;
      const candidateAt = times.get(candidate);
      return candidateAt != null && candidateAt > at;
    });
    if (before < 0) output.push(row);
    else output.splice(before, 0, row);
    if (at != null) {
      times.set(row, at);
      latestAt = Math.max(latestAt, at);
    }
  }
  return output;
}
