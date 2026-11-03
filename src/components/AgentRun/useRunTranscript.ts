import { useEffect } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { useChatSession } from "../Terminal/useChatSession";

export function LiveRunTranscript({ tabId, sessionId, onMessages, onError, onLoaded, onDetached }: {
  tabId: string;
  sessionId: string;
  onMessages: (messages: NormalizedChatMessage[]) => void;
  onError: (error: string | null) => void;
  onLoaded: (loaded: boolean) => void;
  onDetached: (sessionId: string) => void;
}) {
  const tab = useMuxSelector((state) => state.tabs[tabId]);
  const current = tab?.agent_session_id === sessionId;
  const transcript = useChatSession({ tabId, agentSessionId: sessionId,
    sessionBindingNonce: tab?.session_binding_nonce, active: current });
  useEffect(() => {
    if (!current) { onDetached(sessionId); return; }
    onMessages(transcript.allMessages);
    onError(transcript.error);
    onLoaded(transcript.snapshotReady);
  }, [current, transcript.allMessages, transcript.error, transcript.snapshotReady, onMessages, onError, onLoaded, onDetached, sessionId]);
  return null;
}
