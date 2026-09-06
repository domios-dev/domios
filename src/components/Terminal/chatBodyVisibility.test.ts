// Run: npx tsx src/components/Terminal/chatBodyVisibility.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import {
  chatBodyView,
  chatRailsVisible,
  placeChatRails,
  railRowCount,
  type ChatBodyView,
} from "./chatBodyVisibility";

const at = (ms: number | null) => ({ at: ms });
const timeOf = (m: { at: number | null }) => m.at;

const THREE_REPORTS = [1_000, 2_000, 3_000];
const TWO_ASKS = [1_500, 2_500];

function railsFor(view: ChatBodyView, messages: Array<{ at: number | null }>) {
  const placed = placeChatRails({
    view,
    messages,
    messageTimeMs: timeOf,
    reportTimes: THREE_REPORTS,
    askTimes: TWO_ASKS,
  });
  return {
    reports: railRowCount(placed.reports),
    asks: railRowCount(placed.asks),
  };
}

describe("chat body authority", () => {
  it("an empty transcript that is still loading owns the body", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: true,
      bootView: { kind: "none" },
    });
    expect(view.kind).toBe("starting");
  });

  it("a launching agent owns the body even when the chat channel is idle", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: false,
      bootView: { kind: "starting", label: "Starting" },
    });
    expect(view.kind).toBe("starting");
  });

  it("a resolved load over an empty transcript releases the body", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: false,
      bootView: { kind: "none" },
    });
    expect(view.kind).toBe("none");
  });

  it("a transcript is authoritative over a late boot stage", () => {
    const view = chatBodyView({
      hasTranscript: true,
      chatLoading: false,
      bootView: { kind: "starting", label: "Starting" },
    });
    expect(view.kind).toBe("none");
  });
});

describe("chat rail gating", () => {
  it("a loading body produces zero report rows and zero ask rows", () => {
    const view: ChatBodyView = { kind: "starting", label: "Starting" };
    expect(chatRailsVisible(view)).toBe(false);
    expect(railsFor(view, [])).toEqual({ reports: 0, asks: 0 });
  });

  it("a boot overlay produces zero report rows and zero ask rows", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: false,
      bootView: { kind: "starting", label: "Starting" },
    });
    expect(railsFor(view, [])).toEqual({ reports: 0, asks: 0 });
  });

  it("a loading body hides the rails even when the transcript window has messages", () => {
    const view: ChatBodyView = { kind: "starting", label: "Starting" };
    expect(railsFor(view, [at(1_200), at(2_200)])).toEqual({ reports: 0, asks: 0 });
  });

  // The trap in the gate: hiding rails whenever the transcript is empty swaps a
  // cosmetic bug for a functional one.
  it("a resolved load over a genuinely empty transcript still renders every rail row", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: false,
      bootView: { kind: "none" },
    });
    expect(chatRailsVisible(view)).toBe(true);
    expect(railsFor(view, [])).toEqual({
      reports: THREE_REPORTS.length,
      asks: TWO_ASKS.length,
    });
  });

  it("a loaded transcript renders every rail row in its placed position", () => {
    const view = chatBodyView({
      hasTranscript: true,
      chatLoading: false,
      bootView: { kind: "none" },
    });
    const placed = placeChatRails({
      view,
      messages: [at(1_500), at(2_500), at(3_500)],
      messageTimeMs: timeOf,
      reportTimes: THREE_REPORTS,
      askTimes: [],
    });
    expect(railRowCount(placed.reports)).toBe(THREE_REPORTS.length);
    expect(placed.reports.trailing).toEqual([]);
  });
});

finish();
