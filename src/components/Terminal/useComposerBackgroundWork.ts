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
import { runtimeBackgroundProcesses } from "../../lib/tabActivity";
import type { BackgroundProcessAuthority, BackgroundStopRequestStatus, BackgroundStopResult } from "../../lib/backgroundProcesses";
import type { LocalPreviewFile } from "../../../gen/LocalPreviewFile";

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
      return {
        providerWork: tab?.provider_signals?.background,
        rootThreadId,
        bindingNonce,
        agentName: tab?.title?.trim(),
        authority,
        terminals: runtimeBackgroundProcesses(tab),
      };
    }, [tabId]),
    sameJson,
  );
}

function sameAuthority(a: BackgroundProcessAuthority, b: BackgroundProcessAuthority): boolean {
  return a.rootThreadId === b.rootThreadId && a.bindingNonce === b.bindingNonce && a.epoch === b.epoch;
}

export function useComposerBackgroundWork(tabId: string, subagentsVisible: boolean, messages: readonly NormalizedChatMessage[]) {
  const { authority, rootThreadId, bindingNonce, agentName, terminals, providerWork } = useBackgroundProcessState(tabId);
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

  const providerAction = useCallback(<T,>(command: string, item: ComposerBackgroundWorkItem) => {
    if (item.kind !== "shell" || !item.rawId || !rootThreadId || !bindingNonce
      || item.id !== `provider:${rootThreadId}:${item.rawId}`) {
      return Promise.reject(new Error("Background process is unavailable in this session"));
    }
    return invoke<T>(command, {
      tabId,
      request: { sessionId: rootThreadId, bindingNonce, taskId: item.rawId },
    });
  }, [tabId, rootThreadId, bindingNonce]);
  const viewOutput = useCallback(async (item: ComposerBackgroundWorkItem) => {
    if (item.kind !== "shell") return item.outputTail;
    const result = await providerAction<LocalPreviewFile>("agent_provider_background_output", item);
    return result.data;
  }, [providerAction]);
  const askStop = useCallback((item: ComposerBackgroundWorkItem) =>
    providerAction<BackgroundStopRequestStatus>("agent_provider_ask_stop_background_work", item), [providerAction]);

  const computed = withProviderBackgroundWork(
    composerBackgroundWorkItems(messages, terminals, Date.now(), {
      subagentsVisible, rootThreadId, authority: authority ?? undefined,
    }),
    providerWork, rootThreadId, messages,
  );
  // The composer header consumes this through context; unchanged work must keep its identity across transcript renders.
  const itemsRef = useRef(computed);
  if (!sameJson(itemsRef.current, computed)) itemsRef.current = computed;
  const items = itemsRef.current;
  const stopWarning = warning?.tabId === tabId && authority && sameAuthority(warning.authority, authority)
    ? warning.message
    : undefined;
  return useMemo(() => ({ items, stop, viewOutput, askStop, agentName, stopWarning, dismissStopWarning }),
    [items, stop, viewOutput, askStop, agentName, stopWarning, dismissStopWarning]);
}
