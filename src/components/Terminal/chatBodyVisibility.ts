import { placeOutboundReports, type OutboundReportPlacement } from "./outboundReportPlacement";

/** What the chat body is allowed to draw. Exactly one authority, so the view
 *  shows the setup/loading state or the content, never both. */
export type ChatBodyView =
  | { kind: "none" }
  | { kind: "starting"; label: string };

export interface ChatBodyInput {
  hasTranscript: boolean;
  chatLoading: boolean;
  bootView: ChatBodyView;
}

export function chatBodyView(input: ChatBodyInput): ChatBodyView {
  if (input.hasTranscript) return { kind: "none" };
  if (input.chatLoading) return { kind: "starting", label: "Starting" };
  return input.bootView;
}

/** Characterization of the current render: the report and ask rails are emitted
 *  unconditionally, above the setup view, in the same scroll container. */
export function chatRailsVisible(_view: ChatBodyView): boolean {
  return true;
}

function emptyPlacement(): OutboundReportPlacement {
  return { afterIndex: new Map(), trailing: [] };
}

export interface ChatRailPlacements {
  reports: OutboundReportPlacement;
  asks: OutboundReportPlacement;
}

export function placeChatRails<M>(args: {
  view: ChatBodyView;
  messages: M[];
  messageTimeMs: (message: M) => number | null;
  reportTimes: Array<number | null>;
  askTimes: Array<number | null>;
}): ChatRailPlacements {
  if (!chatRailsVisible(args.view)) {
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
