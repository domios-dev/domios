import { timestampMs } from "@codeterm/chat-core";

export interface OutboundReportPlacement {
  /** Report indices to render after a message index; key -1 renders before the first. */
  afterIndex: Map<number, number[]>;
  /** Reports with no time, or newer than every message — the genuine tail. */
  trailing: number[];
}

export function placeOutboundReports<M>(
  messages: M[],
  messageTimeMs: (message: M) => number | null,
  reportTimes: Array<number | null>,
  positions?: { sourceMessageIds: string[]; messageId: (message: M) => string; reportMessageIds: Array<string | null> },
): OutboundReportPlacement {
  const stamped = messages.map((message, index) => ({ index, at: messageTimeMs(message) }));
  const afterIndex = new Map<number, number[]>();
  const trailing: number[] = [];

  const sourceOrder = new Map(positions?.sourceMessageIds.map((id, index) => [id, index]));
  reportTimes.forEach((at, report) => {
    const anchor = positions?.reportMessageIds[report];
    const position = anchor == null ? undefined : sourceOrder.get(anchor);
    if (position != null && positions) {
      let slot = -1;
      for (let index = 0; index < messages.length; index++) {
        const candidate = sourceOrder.get(positions.messageId(messages[index]));
        if (candidate != null && candidate <= position) slot = index;
        else if (candidate != null) break;
      }
      const bucket = afterIndex.get(slot);
      if (bucket) bucket.push(report);
      else afterIndex.set(slot, [report]);
      return;
    }
    if (at == null) {
      trailing.push(report);
      return;
    }
    let slot = -1;
    let hasLater = false;
    for (const message of stamped) {
      if (message.at == null) continue;
      if (message.at <= at) slot = message.index;
      else {
        hasLater = true;
        break;
      }
    }
    if (!hasLater) {
      trailing.push(report);
      return;
    }
    const bucket = afterIndex.get(slot);
    if (bucket) bucket.push(report);
    else afterIndex.set(slot, [report]);
  });

  return { afterIndex, trailing };
}

/** Chat timestamps are provider-shaped: opencode emits numeric epoch-ms strings,
 *  which Date.parse drops. Delegate to the canonical chat-core parser. */
export function messageTimeMs(timestamp: string | undefined | null): number | null {
  return timestampMs(timestamp);
}

/** Daemon report events stamp epoch-seconds while transcript stamps parse to
 *  epoch-ms; sub-1e11 values are seconds (that floor is year 5138 in seconds
 *  but 1973 in ms), so normalize once at ingest — never at comparison sites. */
export function normalizeEpochMs(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  return value < 1e11 ? value * 1000 : value;
}
