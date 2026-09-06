import type { ChatUnavailableReason } from "./chatSessionAuthority";
import { placeOutboundReports, type OutboundReportPlacement } from "./outboundReportPlacement";

/** What the chat body is allowed to draw. Exactly one authority, so the view
 *  shows the setup/loading state or the content, never both. */
export type ChatBodyView =
  | { kind: "none" }
  | { kind: "starting"; label: string }
  | { kind: "failed"; reason: ChatUnavailableReason };

export interface ChatBodyInput {
  hasTranscript: boolean;
  chatLoading: boolean;
  unavailable: ChatUnavailableReason | null;
  bootView: ChatBodyView;
}

export function chatBodyView(input: ChatBodyInput): ChatBodyView {
  if (input.hasTranscript) return { kind: "none" };
  if (input.unavailable) return { kind: "failed", reason: input.unavailable };
  if (input.chatLoading) return { kind: "starting", label: "Starting" };
  return input.bootView;
}

/** The rails are content: they render only once the body has no setup or
 *  loading state left to own, and always once it has none. */
export function chatRailsVisible(view: ChatBodyView): boolean {
  return view.kind === "none";
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
  messages: M[];
  messageTimeMs: (message: M) => number | null;
  reportTimes: Array<number | null>;
  askTimes: Array<number | null>;
}): ChatRailPlacements {
  if (!args.visible) {
    return { reports: emptyPlacement(), asks: emptyPlacement() };
  }
  return {
    reports: placeOutboundReports(args.messages, args.messageTimeMs, args.reportTimes),
    asks: placeOutboundReports(args.messages, args.messageTimeMs, args.askTimes),
  };
}

/** How many rows a rail actually renders — the row count the view emits. */
export function railRowCount(placement: OutboundReportPlacement): number {
  let rows = placement.trailing.length;
  for (const bucket of placement.afterIndex.values()) rows += bucket.length;
  return rows;
}
