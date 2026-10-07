import { useEffect, type RefObject } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "../../lib/bridge";

export function useManagedTerminal(
  termRef: RefObject<Terminal | null>,
  ptyRef: RefObject<string | null>,
  ready: boolean,
  locked: boolean,
  tabId: string,
) {
  useEffect(() => {
    const term = termRef.current;
    if (term) term.options.disableStdin = locked;
  }, [termRef, ready, locked]);
  return () => {
    const ptyId = ptyRef.current;
    if (ptyId) void invoke("interrupt_agent", { ptyId, tabId }).catch((error) => console.warn("Stop generation failed", error));
  };
}
