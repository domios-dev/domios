// Run: npx tsx src/components/Terminal/chatBodyVisibility.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import { firstTurnLocalSends, isLocalSendRow } from "./firstTurnLocalRows";
import { sendRow } from "@codeterm/chat-core";
import {
  chatBodyView,
  chatChannelLoadingRow,
  chatRailsVisible,
  chatRowsPartial,
  placeChatRails,
  railRowCount,
  setupFlowsInLog,
  type ChatBodyView,
} from "./chatBodyVisibility";

const at = (ms: number | null) => ({ at: ms });
const timeOf = (m: { at: number | null }) => m.at;

const THREE_REPORTS = [1_000, 2_000, 3_000];
const TWO_ASKS = [1_500, 2_500];

function railsFor(view: ChatBodyView, messages: Array<{ at: number | null }>) {
  const placed = placeChatRails({
    visible: chatRailsVisible(view),
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
  it("keeps the saved transcript available when loading the session was refused", () => {
    const view = chatBodyView({ restoreLoadRefused: true, snapshotReady: true, hasTranscript: true,
      chatLoading: false, unavailable: null, bootView: { kind: "launch_failed", reason: "opaque" } });
    expect(view.kind).toBe("none");
    expect(chatRailsVisible(view)).toBe(true);
    expect(chatBodyView({ restoreLoadRefused: true, locked: true, hasTranscript: true,
      chatLoading: false, unavailable: null, bootView: view }).kind).toBe("locked");
  });
  it("pending first-turn rendering selects only exact current-attempt local sends", () => {
    const send = { id: "own-send", correlationId: "own-send", content: "opaque", sentAtMs: 1,
      localAttempt: { ptyId: "own-pty", bindingNonce: "own-nonce" } };
    const own = firstTurnLocalSends([send], "own-pty", "own-nonce");
    expect(own.length).toBe(1);
    expect(firstTurnLocalSends([send], "other-pty", "own-nonce").length).toBe(0);
    expect(firstTurnLocalSends([send], "own-pty", "other-nonce").length).toBe(0);
    expect(isLocalSendRow(sendRow(send), own)).toBe(true);
    expect(isLocalSendRow({ ...sendRow(send), sourceSessionId: "foreign-session" }, own)).toBe(false);
    expect(isLocalSendRow({ ...sendRow(send), content: "cached-content" }, own)).toBe(false);
    expect(isLocalSendRow({ ...sendRow(send), id: "cached-row" }, own)).toBe(false);
  });
  it("an undelivered send keeps the body open while its transcript loads", () => {
    const input = { hasUndeliveredSends: true, snapshotReady: false, hasTranscript: false,
      chatLoading: true, unavailable: null, bootView: { kind: "none" as const } };
    expect(chatBodyView(input).kind).toBe("none");
    expect(chatBodyView({ ...input, hasUndeliveredSends: false }).kind).toBe("starting");
    expect(chatBodyView({ ...input, bootView: { kind: "launch_failed", reason: null } }).kind).toBe("launch_failed");
    expect(chatBodyView({ ...input, attachmentError: "opaque" }).kind).toBe("attach_failed");
    expect(chatBodyView({ ...input, unavailable: "no_transcript_source" }).kind).toBe("failed");
  });
  it("setup states that sit beside kept rows flow in the log; an empty launch failure keeps the full body", () => {
    expect(setupFlowsInLog({ kind: "attach_failed", reason: "opaque" }, false, false)).toBe(true);
    expect(setupFlowsInLog({ kind: "failed", reason: "no_transcript_source" }, false, false)).toBe(true);
    expect(setupFlowsInLog({ kind: "none" }, true, false)).toBe(true);
    expect(setupFlowsInLog({ kind: "launch_failed", reason: null }, false, false)).toBe(false);
    expect(setupFlowsInLog({ kind: "launch_failed", reason: null }, false, true)).toBe(true);
    expect(setupFlowsInLog({ kind: "starting", label: "opaque" }, false, true)).toBe(false);
    expect(setupFlowsInLog({ kind: "locked" }, false, true)).toBe(false);
  });
  it("a first-turn-ready composer does not wait for a nonexistent transcript", () => {
    const input = { firstTurnReady: true, snapshotReady: false, hasTranscript: false,
      chatLoading: false, unavailable: null, bootView: { kind: "none" as const } };
    expect(chatBodyView(input).kind).toBe("none");
    expect(chatBodyView({ ...input, hasTranscript: true }).kind).toBe("none");
    expect(chatBodyView({ ...input, firstTurnReady: false }).kind).toBe("starting");
    expect(chatBodyView({ ...input, attachmentError: "opaque-refusal" }).kind).toBe("attach_failed");
    expect(chatBodyView({ ...input, locked: true }).kind).toBe("locked");
  });
  it("reports attachment refusal instead of a loading overlay", () => {
    expect(chatBodyView({ snapshotReady: false, hasTranscript: false, chatLoading: false,
      attachmentError: "request_busy", unavailable: null, bootView: { kind: "none" } }))
      .toEqual({ kind: "attach_failed", reason: "request_busy" });
  });
  it("the lock state owns the body even when the transcript never attached", () => {
    expect(chatBodyView({ locked: true, snapshotReady: false, hasTranscript: false, chatLoading: true,
      attachmentError: "refused", unavailable: null, bootView: { kind: "starting", label: "opaque" } }))
      .toEqual({ kind: "locked" });
  });
  it("cached or tracked rows cannot paint before the first authoritative snapshot", () => {
    const view = chatBodyView({ snapshotReady: false, hasTranscript: true, chatLoading: true,
      unavailable: null, bootView: { kind: "none" } });
    expect(view.kind).toBe("starting");
    expect(chatRailsVisible(view)).toBe(false);
  });
  it("renders a held authority window while the attach is still loading", () => {
    const view = chatBodyView({ snapshotReady: false, hasHeldWindow: true, hasTranscript: true,
      chatLoading: true, unavailable: null, bootView: { kind: "starting", label: "opaque" } });
    expect(view.kind).toBe("none");
    expect(chatRailsVisible(view)).toBe(true);
    expect(chatChannelLoadingRow(true, view, false)).toBe("model");
    expect(chatChannelLoadingRow(true, view, true, true)).toBe("model");
  });
  it("an already loaded conversation remains visible during channel recovery", () => {
    const view = chatBodyView({ snapshotReady: true, hasTranscript: true, chatLoading: true,
      unavailable: null, bootView: { kind: "none" } });
    expect(view.kind).toBe("none");
  });
  it("a loaded empty snapshot remains ready while reconnecting", () => {
    expect(chatBodyView({ snapshotReady: true, hasTranscript: false, chatLoading: true,
      unavailable: null, bootView: { kind: "starting", label: "opaque" } }).kind).toBe("none");
  });
  it("spawn failure owns the body even when cached or tracked rows exist", () => {
    const failed: ChatBodyView = { kind: "launch_failed", reason: "opaque-refusal" };
    expect(chatBodyView({ hasTranscript: true, chatLoading: true, unavailable: null, bootView: failed })).toEqual(failed);
    expect(chatRailsVisible(failed)).toBe(false);
  });
  it("chatLoading with bootView none is not starting", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: true,
      unavailable: null,
      bootView: { kind: "none" },
    });
    expect(view.kind).toBe("none");
  });

  it("a launching agent owns the body even when the chat channel is idle", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: false,
      unavailable: null,
      bootView: { kind: "starting", label: "Starting" },
    });
    expect(view.kind).toBe("starting");
  });

  it("a resolved load over an empty transcript releases the body", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: false,
      unavailable: null,
      bootView: { kind: "none" },
    });
    expect(view.kind).toBe("none");
  });

  it("a transcript is authoritative over a late boot stage", () => {
    const view = chatBodyView({
      hasTranscript: true,
      chatLoading: false,
      unavailable: null,
      bootView: { kind: "starting", label: "Starting" },
    });
    expect(view.kind).toBe("none");
  });
});

describe("chat channel status", () => {
  it("shows channel recovery alongside an authoritative transcript", () => {
    const view = chatBodyView({ hasTranscript: true, chatLoading: true, unavailable: null, bootView: { kind: "none" } });
    expect(chatChannelLoadingRow(true, view, false)).toBe("model");
    expect(chatChannelLoadingRow(true, view, true)).toBe(null);
  });
  it("hides channel status after loading completes", () => {
    expect(chatChannelLoadingRow(false, { kind: "none" }, false)).toBe(null);
  });
  it("shows a loading row for an empty transcript", () => {
    const view = chatBodyView({ hasTranscript: false, chatLoading: true, unavailable: null, bootView: { kind: "none" } });
    expect(chatChannelLoadingRow(true, view, false)).toBe("model");
  });
  it("leaves a reported failure to its body indicator", () => {
    expect(chatChannelLoadingRow(true, { kind: "failed", reason: "no_transcript_source" }, false)).toBe(null);
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
      unavailable: null,
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
      unavailable: null,
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
      unavailable: null,
      bootView: { kind: "none" },
    });
    const placed = placeChatRails({
      visible: chatRailsVisible(view),
      messages: [at(1_500), at(2_500), at(3_500)],
      messageTimeMs: timeOf,
      reportTimes: THREE_REPORTS,
      askTimes: [],
    });
    expect(railRowCount(placed.reports)).toBe(THREE_REPORTS.length);
    expect(placed.reports.trailing).toEqual([]);
  });
});

describe("chat loader terminal state", () => {
  it("a bound session the host has no transcript source for is reported, not awaited", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: true,
      unavailable: "no_transcript_source",
      bootView: { kind: "none" },
    });
    expect(view).toEqual({ kind: "failed", reason: "no_transcript_source" });
  });

  it("a snapshot the tab cannot adopt is reported rather than left spinning", () => {
    const view = chatBodyView({
      hasTranscript: false,
      chatLoading: true,
      unavailable: "authority_mismatch",
      bootView: { kind: "starting", label: "Starting" },
    });
    expect(view.kind).toBe("failed");
  });

  it("a reported failure produces zero report rows and zero ask rows", () => {
    const view: ChatBodyView = { kind: "failed", reason: "no_transcript_source" };
    expect(railsFor(view, [])).toEqual({ reports: 0, asks: 0 });
  });

  it("an observed delta clears the reported failure and returns the body to content", () => {
    const view = chatBodyView({
      hasTranscript: true,
      chatLoading: false,
      unavailable: "no_transcript_source",
      bootView: { kind: "none" },
    });
    expect(view.kind).toBe("none");
  });
});

describe("chatRowsPartial", () => {
  const none: ChatBodyView = { kind: "none" };
  const attached = { snapshotReady: true, hasHeldWindow: false, firstTurnReady: false };
  it("a relaunch that has not reattached its transcript draws only part of it", () => {
    expect(chatRowsPartial(none, { ...attached, snapshotReady: false })).toBe(true);
    expect(chatRowsPartial({ kind: "starting", label: "opaque" }, attached)).toBe(true);
    expect(chatRowsPartial({ kind: "launch_failed", reason: null }, attached)).toBe(true);
  });
  it("an attached transcript, a held window and a first turn draw every row they have", () => {
    expect(chatRowsPartial(none, attached)).toBe(false);
    expect(chatRowsPartial(none, { ...attached, snapshotReady: false, hasHeldWindow: true })).toBe(false);
    expect(chatRowsPartial(none, { ...attached, snapshotReady: false, firstTurnReady: true })).toBe(false);
  });
});

finish();
