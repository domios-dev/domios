import { useEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import {
  applyShellLifecycle,
  shellSnapshotToMessages,
  type CommandLifecycle,
  type NormalizedChatMessage as ChatMessage,
  type ShellCommandSnapshot,
} from "@codeterm/chat-core";

interface ShellCommandEvent {
  tab_id: string;
  event: CommandLifecycle;
}

/** Shell-integration command items for a tab (OSC 133/633/7). Hydrates the
 *  existing command list from the snapshot on mount, then folds live
 *  `shell-command-changed` events — the SAME emit_everywhere channel
 *  tab-status-changed uses — through the shared chat-core reducer. */
export function useShellCommands(tabId: string): ChatMessage[] {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  // every appended bubble gets a stable, unique id.
  const seqRef = useRef(0);
  const nextSeq = () => {
    seqRef.current += 1;
    return seqRef.current;
  };

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    setMessages([]);
    seqRef.current = 0;

    (async () => {
      try {
        const snap = await invoke<ShellCommandSnapshot>("shell_command_snapshot", { tabId });
        if (cancelled) return;
        const base = Date.now();
        setMessages(shellSnapshotToMessages(snap ?? { commands: [] }, `shell-${tabId}`, base));
      } catch {
        /* no PTY / no markers yet — start empty, live events still flow */
      }
      if (cancelled) return;
      unlisten = await listen<ShellCommandEvent>("shell-command-changed", (ev) => {
        if (cancelled || ev.payload.tab_id !== tabId) return;
        setMessages((prev) =>
          applyShellLifecycle(prev, ev.payload.event, {
            id: `shell-${tabId}-live-${nextSeq()}`,
            timestampMs: Date.now(),
          }),
        );
      });
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [tabId]);

  return messages;
}
