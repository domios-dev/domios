/**
 * A sent report is never in the sender's own provider transcript, so it carries no
 * position in it. Rendering the cards after the transcript pins them to the bottom,
 * and every message that arrives afterwards appears above them — the card reads as
 * stuck and the stream reads out of order. Place each card by time instead, and only
 * let it trail when nothing newer exists to place it before.
 */
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
): OutboundReportPlacement {
  const stamped = messages.map((message, index) => ({ index, at: messageTimeMs(message) }));
  const afterIndex = new Map<number, number[]>();
  const trailing: number[] = [];

  reportTimes.forEach((at, report) => {
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

/** Chat message timestamps are ISO strings and optional; unknown time must not
 *  reorder anything, so it reads as "no position". */
export function messageTimeMs(timestamp: string | undefined | null): number | null {
  if (!timestamp) return null;
  const parsed = Date.parse(timestamp);
  return Number.isNaN(parsed) ? null : parsed;
}
