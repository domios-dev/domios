import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "../../lib/bridge";
import type { TabStatus } from "../../lib/types";
import type { ChatMsgEvent } from "./chatSessionAuthority";
import { listenForTestChatGrowth, listenForTestTabStatus } from "./chatTestBridge";
import {
  initialSessionQuietState,
  reduceSessionQuiet,
  sessionQuietEventFromChat,
  type SessionQuietState,
} from "./sessionQuietDetection";

interface TabStatusEvent {
  tab_id: string;
  status: TabStatus;
}

function isTabStatus(status: unknown): status is TabStatus {
  return status === "Working" || status === "Waiting" || status === "Idle" || status === "Dead";
}

export function useSessionQuietDetection(tabId: string, sessionId: string | null): SessionQuietState {
  const [state, setState] = useState(() => initialSessionQuietState(tabId, sessionId));
  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => {
    let cancelled = false;
    const reset = initialSessionQuietState(tabId, sessionId);
    stateRef.current = reset;
    setState(reset);

    const apply = (next: SessionQuietState) => {
      if (cancelled) return;
      stateRef.current = next;
      setState(next);
    };
    const unlisteners: UnlistenFn[] = [];
    const testChatUnlisten = listenForTestChatGrowth(tabId, (payload) => {
      apply(reduceSessionQuiet(stateRef.current, sessionQuietEventFromChat(tabId, payload)));
    });
    const testStatusUnlisten = listenForTestTabStatus(tabId, (status) => {
      apply(reduceSessionQuiet(stateRef.current, {
        kind: "tab-status",
        tabId,
        status,
      }));
    });
    if (testChatUnlisten) unlisteners.push(testChatUnlisten);
    if (testStatusUnlisten) unlisteners.push(testStatusUnlisten);
    void Promise.all([
      listen<ChatMsgEvent>(`tab-chat-msg-${tabId}`, (event) => {
        apply(reduceSessionQuiet(stateRef.current, sessionQuietEventFromChat(tabId, event.payload)));
      }),
      listen<TabStatusEvent>("tab-status-changed", (event) => {
        const payload = event.payload;
        if (payload?.tab_id !== tabId || !isTabStatus(payload.status)) return;
        apply(reduceSessionQuiet(stateRef.current, {
          kind: "tab-status",
          tabId,
          status: payload.status,
        }));
      }),
    ]).then((nextUnlisteners) => {
      if (cancelled) {
        for (const unlisten of nextUnlisteners) unlisten();
        return;
      }
      unlisteners.push(...nextUnlisteners);
    }).catch(() => undefined);

    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, [sessionId, tabId]);

  return state;
}
