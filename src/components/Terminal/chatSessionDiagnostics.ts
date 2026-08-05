export type ChatSessionDiagnosticEvent =
  | {
      edge: "event_arrival" | "authority_adopt" | "authority_drop" | "state_enqueued";
      tabId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
      messageIds: string[];
      tailSignature: string | null;
    }
  | {
      edge: "snapshot_request";
      tabId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
    }
  | {
      edge: "snapshot_arrival";
      tabId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
      messageIds: string[];
      subscriptionId: string | null;
      /** Backend-sourced recovery coordinates so a re-attach after deactivation is
       *  causally attributable: the tailer's cursor/offset plus the transcript tail
       *  signature the snapshot recovered to. */
      tailSignature: string | null;
      cursor: string | null;
      offset: number | null;
      startOffset: number | null;
    }
  | {
      edge: "state_commit";
      tabId: string;
      sourceSessionId: string | null;
      sessionBindingNonce: string | null;
      messageIds: string[];
      tailSignature: string | null;
    }
  | {
      edge: "listener_owned" | "listener_released";
      tabId: string;
    }
  | {
      edge: "subscription_owned" | "subscription_released";
      tabId: string;
      subscriptionId: string;
    };

export type ChatSessionDiagnosticSink = (event: ChatSessionDiagnosticEvent) => void;

/** Tail signature of a message list for projection diagnostics: last id plus the
 *  last message's content length. Streaming grows the tail message under a stable
 *  id, so an id-only tail would make a committed-but-unpainted stream update
 *  invisible to the stall detector — the content length restores that progress. */
export function paintTailSignature(
  last: { id: string; content?: string | null } | undefined | null,
): string | null {
  return last ? `${last.id}#${last.content?.length ?? 0}` : null;
}

export function tailSignatureOf(
  messages: readonly { id: string; content?: string | null }[],
): string | null {
  return paintTailSignature(messages[messages.length - 1]);
}
