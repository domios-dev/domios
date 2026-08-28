import type { ChatMsgEvent } from "./chatSessionAuthority";
import type { TabStatus } from "../../lib/types";

export type SessionQuietEvent =
  | { kind: "transcript-growth"; tabId: string; sourceSessionId: string | null; hasMessages: boolean }
  | { kind: "tab-status"; tabId: string; status: TabStatus };

export interface SessionQuietState {
  tabId: string;
  sessionId: string | null;
  status: TabStatus | null;
  sawGrowth: boolean;
  workingObservationSeen: boolean;
  quiet: boolean;
}

export function initialSessionQuietState(tabId: string, sessionId: string | null): SessionQuietState {
  return { tabId, sessionId, status: null, sawGrowth: false, workingObservationSeen: false, quiet: false };
}

function isWorking(status: TabStatus): boolean {
  return status === "Working";
}

export function reduceSessionQuiet(
  state: SessionQuietState,
  event: SessionQuietEvent,
): SessionQuietState {
  if (event.tabId !== state.tabId) return state;

  if (event.kind === "transcript-growth") {
    if (!event.hasMessages || event.sourceSessionId == null || event.sourceSessionId !== state.sessionId) {
      return state;
    }
    return { ...state, sawGrowth: true, quiet: false };
  }

  if (!isWorking(event.status)) {
    return { ...state, status: event.status, sawGrowth: false, workingObservationSeen: false, quiet: false };
  }

  const quiet = state.sessionId !== null && state.workingObservationSeen && !state.sawGrowth;
  return { ...state, status: event.status, workingObservationSeen: true, quiet };
}

export function sessionQuietEventFromChat(tabId: string, event: ChatMsgEvent): SessionQuietEvent {
  return {
    kind: "transcript-growth",
    tabId,
    sourceSessionId: event.source_session_id,
    hasMessages: event.tab_id === tabId && event.messages.length > 0,
  };
}
