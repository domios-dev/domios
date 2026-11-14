import type { ChatUnavailableReason } from "./chatSessionAuthority";
import { placeOutboundReports, type OutboundReportPlacement } from "./outboundReportPlacement";

/** What the chat body is allowed to draw. Exactly one authority, so the view
 *  shows the setup/loading state or the content, never both. */
export type ChatBodyView =
  | { kind: "none" }
  | { kind: "starting"; label: string }
  | { kind: "launch_failed"; reason: string | null }
  | { kind: "attach_failed"; reason: string }
  | { kind: "locked" }
  | { kind: "failed"; reason: ChatUnavailableReason };

export interface ChatBodyInput {
  firstTurnReady?: boolean;
  snapshotReady?: boolean;
  hasHeldWindow?: boolean;
  hasUndeliveredSends?: boolean;
  hasTranscript: boolean;
  chatLoading: boolean;
  unavailable: ChatUnavailableReason | null;
  bootView: ChatBodyView;
  attachmentError?: string | null;
  locked?: boolean;
  restoreLoadRefused?: boolean;
}

export function chatBodyView(input: ChatBodyInput): ChatBodyView {
  if (input.locked) return { kind: "locked" };
  if (input.restoreLoadRefused) return { kind: "none" };
  if (input.bootView.kind === "launch_failed") return input.bootView;
  if (input.attachmentError) return { kind: "attach_failed", reason: input.attachmentError };
  if ((input.firstTurnReady || input.hasUndeliveredSends) && !input.unavailable) return { kind: "none" };
  if (input.hasHeldWindow) return { kind: "none" };
  if (input.snapshotReady === false) {
    return input.unavailable ? { kind: "failed", reason: input.unavailable } : { kind: "starting", label: "Loading conversation" };
  }
  if (input.snapshotReady === true) return { kind: "none" };
  if (input.hasTranscript) return { kind: "none" };
  if (input.unavailable) return { kind: "failed", reason: input.unavailable };
  return input.bootView;
}

/** The rails are content: they render only once the body has no setup or
 *  loading state left to own, and always once it has none. */
export function chatRailsVisible(view: ChatBodyView): boolean {
  return view.kind === "none";
}

/** Only part of the transcript is drawn while a relaunch reattaches it: local send rows, without the rows they follow. */
export function chatRowsPartial(view: ChatBodyView, facts: { snapshotReady: boolean; hasHeldWindow: boolean; firstTurnReady: boolean }): boolean {
  return view.kind !== "none" || (!facts.snapshotReady && !facts.hasHeldWindow && !facts.firstTurnReady);
}

/** A setup state beside rows takes a row of the log, so it and they can never cover each other. */
export function setupFlowsInLog(view: ChatBodyView, restoreLoadRefused: boolean, hasRows: boolean): boolean {
  return restoreLoadRefused || view.kind === "attach_failed" || view.kind === "failed"
    || (view.kind === "launch_failed" && hasRows);
}

export function chatChannelLoadingRow(loading: boolean, view: ChatBodyView, snapshotReady: boolean, hasHeldWindow = false): "model" | null {
  return loading && (!snapshotReady || hasHeldWindow) && view.kind === "none" ? "model" : null;
}

function emptyPlacement(): OutboundReportPlacement {
  return { afterIndex: new Map(), trailing: [] };
}

export interface ChatRailPlacements {
  reports: OutboundReportPlacement;
  asks: OutboundReportPlacement;
}

export function placeChatRails<M>(args: {
  visible: boolean;
  /** Transcript rows exist above the drawn window, hidden or not yet loaded. */
  hiddenBefore: boolean;
  /** Newer transcript rows exist below the drawn window. */
  hiddenAfter: boolean;
  messages: M[];
  messageTimeMs: (message: M) => number | null;
  reportTimes: Array<number | null>;
  reportPositions?: { sourceMessageIds: string[]; messageId: (message: M) => string; reportMessageIds: Array<string | null> };
  askTimes: Array<number | null>;
}): ChatRailPlacements {
  if (!args.visible) {
    return { reports: emptyPlacement(), asks: emptyPlacement() };
  }
  const bounds = { hiddenBefore: args.hiddenBefore, hiddenAfter: args.hiddenAfter };
  return {
    reports: placeOutboundReports(args.messages, args.messageTimeMs, args.reportTimes, args.reportPositions, bounds),
    asks: placeOutboundReports(args.messages, args.messageTimeMs, args.askTimes, undefined, bounds),
  };
}

/** How many rows a rail actually renders — the row count the view emits. */
export function railRowCount(placement: OutboundReportPlacement): number {
  let rows = placement.trailing.length;
  for (const bucket of placement.afterIndex.values()) rows += bucket.length;
  return rows;
}

export function composerVisible(view: ChatBodyView): boolean {
  return view.kind !== "launch_failed";
}
