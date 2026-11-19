import { useCallback, useMemo, useRef, useState } from "react";
import { sameJson } from "../../lib/sameJson";
import { invoke } from "../../lib/bridge";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import type { ExtendedState } from "../../store/multiplexer/tree";
import {
  composerBackgroundWorkItems,
  withProviderBackgroundWork,
  type ComposerBackgroundWorkItem,
} from "./composerBackgroundWorkModel";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { BackgroundProcessAuthority, BackgroundStopResult } from "../../lib/backgroundProcesses";

function useBackgroundProcessState(tabId: string) {
  return useMuxSelector(
    useCallback((state: ExtendedState) => {
      const tab = state.tabs[tabId];
      const rootThreadId = tab?.agent_session_id ?? undefined;
      const bindingNonce = tab?.session_binding_nonce;
      const runtime = tab?.agent_runtime_state;
      const authority = rootThreadId && bindingNonce && runtime?.state === "loaded"
        ? { rootThreadId, bindingNonce, epoch: runtime.epoch }
        : null;
      const roster = tab?.remote_channels && tab.remote_channels.control !== "available"
        ? undefined
        : runtime?.backgroundProcesses;
      return {
        providerWork: tab?.provider_signals?.background?.running ?? {},
        unknownWork: tab?.provider_signals?.background?.unknown ?? {},
        rootThreadId,
        authority,
        terminals: roster && authority && sameAuthority(roster, authority) ? roster.processes : [],
      };
    }, [tabId]),
    sameJson,
  );
}

function sameAuthority(a: BackgroundProcessAuthority, b: BackgroundProcessAuthority): boolean {
  return a.rootThreadId === b.rootThreadId && a.bindingNonce === b.bindingNonce && a.epoch === b.epoch;
}

export function useComposerBackgroundWork(tabId: string, subagentsVisible: boolean, messages: readonly NormalizedChatMessage[]) {
  const { authority, rootThreadId, terminals, providerWork, unknownWork } = useBackgroundProcessState(tabId);
  const [warning, setWarning] = useState<{ tabId: string; authority: BackgroundProcessAuthority; message: string }>();
  const dismissStopWarning = useCallback(() => setWarning(undefined), []);
  const stop = useCallback(async (item: ComposerBackgroundWorkItem) => {
    if (item.kind !== "terminal" || !authority || !item.processId) return;
    const result = await invoke<BackgroundStopResult>("agent_runtime_stop_background_process", {
      tabId,
      request: { ...authority, threadId: item.threadId, processId: item.processId },
    });
    if (result.status === "degraded") {
      const message = result.reason === "attributionUnavailable"
        ? `Could not identify the OS processes for ${item.name}.`
        : `Could not confirm that the OS processes for ${item.name} exited.`;
      setWarning({ tabId, authority, message });
    }
  }, [authority, tabId]);

  const computed = withProviderBackgroundWork(
    composerBackgroundWorkItems(messages, terminals, Date.now(), {
      subagentsVisible, rootThreadId, authority: authority ?? undefined,
    }),
    providerWork, rootThreadId, unknownWork, messages,
  );
  // The composer header consumes this through context; unchanged work must keep its identity across transcript renders.
  const itemsRef = useRef(computed);
  if (!sameJson(itemsRef.current, computed)) itemsRef.current = computed;
  const items = itemsRef.current;
  const stopWarning = warning?.tabId === tabId && authority && sameAuthority(warning.authority, authority)
    ? warning.message
    : undefined;
  return useMemo(() => ({ items, stop, stopWarning, dismissStopWarning }), [items, stop, stopWarning, dismissStopWarning]);
}
