import { describe, expect, it } from "vitest";
import type { TranscriptForward, TranscriptRow } from "../../lib/transport/liveTranscript";
import { groupPassages } from "./liveTranscriptPassages";

function speech(seq: number, atMs: number, text = "Four words per segment"): TranscriptRow {
  return { seq, atMs, content: { kind: "speech", text } };
}

function forward(fromSeq: number, toSeq: number): TranscriptForward {
  return {
    forwardId: String(fromSeq), range: { fromSeq, toSeq }, atMs: 90_000,
    targetTabId: "tab", targetTitle: "Fixture", sentText: "Request", sentExcerpt: "Request",
  };
}

function segments(items: ReturnType<typeof groupPassages>) {
  return items.map(item => item.kind === "passage" ? item.segments.map(segment => segment.seq) : item.kind);
}

describe("groupPassages", () => {
  it("joins speech at an eight second gap and splits beyond it", () => {
    expect(segments(groupPassages([speech(1, 0), speech(2, 8_000), speech(3, 16_001)], [], null)))
      .toEqual([[1, 2], [3]]);
  });

  it("keeps one forward across gaps and splits at each forward or routing state boundary", () => {
    const rows = [speech(1, 0), speech(2, 200), speech(3, 9_000), speech(4, 10_000), speech(5, 11_000)];
    const first = forward(2, 3);
    const second = forward(4, 4);
    const items = groupPassages(rows, [first, second], 3);
    expect(segments(items)).toEqual([[1], [2, 3], [4], [5]]);
    expect(items).toMatchObject([{ state: "not_sent" }, { state: "sent", forward: first },
      { state: "sent", forward: second }, { state: "pending" }]);
  });

  it("merges a short not-sent remark into either adjacent not-sent passage", () => {
    const rows = [speech(1, 0, "Uh. Okay."), speech(2, 9_000), speech(3, 18_000, "I see now."), speech(4, 27_000)];
    expect(segments(groupPassages(rows, [], 4))).toEqual([[1, 2, 3], [4]]);
  });

  it("collapses only runs of at least three remarks older than the latest sent passage", () => {
    const rows = Array.from({ length: 10 }, (_, index) => speech(index + 1, index * 9_000));
    const items = groupPassages(rows, [forward(3, 3), forward(7, 7)], 10);
    expect(segments(items)).toEqual([[1], [2], [3], "collapsed_not_sent", [7], [8], [9], [10]]);
    const collapsed = items[3];
    expect(collapsed.kind === "collapsed_not_sent" && segments(collapsed.passages)).toEqual([[4], [5], [6]]);
    expect(segments(groupPassages(rows, [], 10))).toEqual(rows.map(row => [row.seq]));
  });

  it("attaches provisional speech to a trailing pending passage or creates a draft-only passage", () => {
    const rows = [speech(1, 0), speech(2, 100)];
    expect(groupPassages(rows, [], 1, { provisional: "Next words" })).toMatchObject([
      { state: "not_sent" }, { state: "pending", segments: [{ seq: 2 }], provisional: "Next words" },
    ]);
    expect(groupPassages(rows, [], 2, { provisional: "Next words" })).toMatchObject([
      { state: "not_sent" }, { state: "pending", segments: [], provisional: "Next words" },
    ]);
  });

  it("keeps reviewed speech stable as committed and provisional speech arrive", () => {
    const rows = [speech(1, 0), speech(2, 200)];
    const reviewing = groupPassages(rows, [], null, { reviewingThroughSeq: 2, provisional: "Draft" });
    expect(segments(reviewing)).toEqual([[1, 2], []]);
    const extended = groupPassages([...rows, speech(3, 400)], [], null, { reviewingThroughSeq: 2, provisional: "Draft" });
    expect(extended[0]).toEqual(reviewing[0]);
    expect(extended[1]).toMatchObject({ state: "pending", segments: [{ seq: 3 }], provisional: "Draft" });
  });

  it("breaks at pause and resume markers without merging short remarks across them", () => {
    const rows: TranscriptRow[] = [
      speech(1, 0, "Uh."),
      { seq: 2, atMs: 100, content: { kind: "paused", reason: "manual" } },
      { seq: 3, atMs: 200, content: { kind: "resumed" } },
      speech(4, 300, "Okay."),
    ];
    expect(segments(groupPassages(rows, [], 4))).toEqual([[1], "paused", "resumed", [4]]);
    expect(segments(groupPassages(rows, [forward(1, 4)], 4))).toEqual([[1], "paused", "resumed", [4]]);
  });
});
