import type { TranscriptForward, TranscriptRow } from "../../lib/transport/liveTranscript";

type Segment = { seq: number; text: string; atMs: number };
type PassageState = "pending" | "sent" | "not_sent";
type Marker = { kind: "paused" | "resumed"; id: string; atMs: number; resumedAtMs?: number };

export type TranscriptPassage = {
  kind: "passage";
  id: string;
  state: PassageState;
  segments: Segment[];
  forward?: TranscriptForward;
  forwards?: TranscriptForward[];
  provisional?: string;
};

type ThreadItem = TranscriptPassage | Marker | {
  kind: "collapsed_not_sent";
  id: string;
  passages: TranscriptPassage[];
};

function sameForwards(left: readonly TranscriptForward[] | undefined, right: readonly TranscriptForward[]) {
  const a = left ?? [];
  return a.length === right.length && a.every((item, index) => item.forwardId === right[index].forwardId);
}

function isNotSent(item: TranscriptPassage | Marker | undefined): item is TranscriptPassage & { state: "not_sent" } {
  return item?.kind === "passage" && item.state === "not_sent";
}

function shortRemark(passage: TranscriptPassage) {
  return passage.segments.map(segment => segment.text).join(" ").trim().split(/\s+/u).length <= 3;
}

function mergeShortRemarks(items: (TranscriptPassage | Marker)[]) {
  const merged: (TranscriptPassage | Marker)[] = [];
  for (const item of items) {
    const previous = merged[merged.length - 1];
    if (isNotSent(item) && isNotSent(previous) && (shortRemark(item) || shortRemark(previous))) {
      previous.segments.push(...item.segments);
    } else {
      merged.push(item);
    }
  }
  return merged;
}

export function groupPassages(
  rows: readonly TranscriptRow[],
  forwards: readonly TranscriptForward[] | null | undefined,
  ackedSeq: number | null | undefined,
  { reviewingThroughSeq, provisional }: { reviewingThroughSeq?: number | null; provisional?: string } = {},
): ThreadItem[] {
  const items: (TranscriptPassage | Marker)[] = [];
  for (const { seq, atMs, content } of rows) {
    if (content.kind !== "speech") {
      const previous = items[items.length - 1];
      if (previous?.kind === "paused") {
        if (content.kind === "resumed") previous.resumedAtMs = atMs;
        else delete previous.resumedAtMs;
      } else if (previous?.kind === "resumed") {
        if (content.kind === "resumed") previous.atMs = atMs;
        else items[items.length - 1] = { kind: "paused", id: `marker-${seq}`, atMs };
      } else {
        items.push({ kind: content.kind, id: `marker-${seq}`, atMs });
      }
      continue;
    }
    const matching = forwards?.filter(({ range }) => range.fromSeq <= seq && seq <= range.toSeq) ?? [];
    const forward = matching[0];
    const state = forward ? "sent" : ackedSeq != null && seq <= ackedSeq ? "not_sent" : "pending";
    const previous = items[items.length - 1];
    const last = previous?.kind === "passage" ? previous.segments[previous.segments.length - 1] : undefined;
    const crossedReview = state === "pending" && reviewingThroughSeq != null && last &&
      last.seq <= reviewingThroughSeq && seq > reviewingThroughSeq;
    const segment = { seq, text: content.text, atMs };
    if (previous?.kind === "passage" && last && previous.state === state && sameForwards(previous.forwards, matching) &&
      (forward || atMs - last.atMs <= 8000) && !crossedReview) {
      previous.segments.push(segment);
    } else {
      items.push({
        kind: "passage", id: `speech-${seq}`, state, forward,
        forwards: matching.length ? matching : undefined, segments: [segment],
      });
    }
  }

  const merged = mergeShortRemarks(items);
  if (provisional) {
    const last = merged[merged.length - 1];
    const coveredByReview = reviewingThroughSeq != null && (last?.kind === "passage" &&
      last.segments[last.segments.length - 1].seq <= reviewingThroughSeq);
    if (last?.kind === "passage" && last.state === "pending" && !coveredByReview) {
      last.provisional = provisional;
    } else {
      merged.push({ kind: "passage", id: "draft", state: "pending", segments: [], provisional });
    }
  }

  let latestSent = merged.length - 1;
  while (latestSent >= 0) {
    const item = merged[latestSent];
    if (item.kind === "passage" && item.state === "sent") break;
    latestSent--;
  }
  const thread: ThreadItem[] = [];
  for (let index = 0; index < merged.length;) {
    const item = merged[index++];
    if (!isNotSent(item) || index > latestSent) {
      thread.push(item);
      continue;
    }
    const run = [item];
    let next = merged[index];
    while (isNotSent(next)) {
      run.push(next);
      next = merged[++index];
    }
    if (run.length >= 3) thread.push({ kind: "collapsed_not_sent", id: `not-sent-${item.id}`, passages: run });
    else thread.push(...run);
  }
  return thread;
}
