import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import { useMuxSelector } from "../../store/multiplexer";
import type { ExtendedState } from "../../store/multiplexer/tree";
import {
  composerBackgroundWorkItems,
  type ComposerBackgroundWorkItem,
} from "./composerBackgroundWorkModel";
import { useComposerBackgroundWorkMessages } from "./ComposerBackgroundWorkContext";
import type { BackgroundProcessAuthority, BackgroundProcessSnapshot, BackgroundProcessEntry } from "../../lib/backgroundProcesses";

interface BackgroundProcessState {
  identity: BackgroundProcessAuthority | null;
  refreshKey: string;
}

interface LoadedProcesses {
  identity: BackgroundProcessAuthority | null;
  processes: BackgroundProcessEntry[];
}

function sameAuthority(a: BackgroundProcessAuthority | null, b: BackgroundProcessAuthority | null): boolean {
  return a?.rootThreadId === b?.rootThreadId && a?.bindingNonce === b?.bindingNonce && a?.epoch === b?.epoch;
}

function useBackgroundProcessState(tabId: string) {
  return useMuxSelector(
    useCallback((state: ExtendedState): BackgroundProcessState => {
      const tab = state.tabs[tabId];
      const rootThreadId = tab?.agent_session_id;
      const bindingNonce = tab?.session_binding_nonce;
      const epoch = tab?.agent_runtime_state?.epoch;
      return {
        identity: rootThreadId && bindingNonce && epoch !== undefined ? { rootThreadId, bindingNonce, epoch } : null,
        refreshKey: JSON.stringify([tab?.agent_runtime_state?.active_turn_started_at_ms, tab?.agent_runtime_state?.last_turn_duration_ms]),
      };
    }, [tabId]),
    (a, b) => sameAuthority(a.identity, b.identity) && a.refreshKey === b.refreshKey,
  );
}

function workRevision(messages: ReturnType<typeof useComposerBackgroundWorkMessages>): string {
  return JSON.stringify(messages.flatMap((message) => {
    if (message.type !== "tool_call") return [];
    if (message.toolKind === "agent" && message.toolName === "agent_activity") {
      return [[message.id, message.timestamp, message.toolInput]];
    }
    return message.toolStatus === "running" ? [[message.id, message.timestamp, message.toolStatus]] : [];
  }));
}

function validSnapshot(snapshot: BackgroundProcessSnapshot, identity: BackgroundProcessAuthority): boolean {
  return snapshot.rootThreadId === identity.rootThreadId
    && snapshot.bindingNonce === identity.bindingNonce
    && snapshot.epoch === identity.epoch;
}

export function useComposerBackgroundWork(tabId: string, supported: boolean, subagentsVisible: boolean) {
  const messages = useComposerBackgroundWorkMessages();
  const { identity, refreshKey } = useBackgroundProcessState(tabId);
  const revision = workRevision(messages);
  const [loaded, setLoaded] = useState<LoadedProcesses>({ identity: null, processes: [] });
  const [error, setError] = useState<string | null>(null);

  const acceptSnapshot = useCallback((snapshot: BackgroundProcessSnapshot, expected: BackgroundProcessAuthority) => {
    if (!validSnapshot(snapshot, expected)) throw new Error("Background process binding changed");
    setLoaded({ identity: expected, processes: snapshot.processes });
    setError(null);
  }, []);

  const rootThreadId = identity?.rootThreadId;
  const bindingNonce = identity?.bindingNonce;
  const epoch = identity?.epoch;
  useEffect(() => {
    if (!supported || rootThreadId === undefined || bindingNonce === undefined || epoch === undefined) {
      setLoaded({ identity: null, processes: [] });
      setError(null);
      return;
    }
    const request: BackgroundProcessAuthority = { rootThreadId, bindingNonce, epoch };
    const controller = new AbortController();
    void invoke<BackgroundProcessSnapshot>("agent_runtime_background_processes", {
      tabId,
      request,
      signal: controller.signal,
    }).then((snapshot) => {
      if (!controller.signal.aborted) acceptSnapshot(snapshot, request);
    }).catch((reason: unknown) => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
    });
    return () => controller.abort();
  }, [tabId, supported, rootThreadId, bindingNonce, epoch, refreshKey, revision, acceptSnapshot]);

  const stop = useCallback(async (item: ComposerBackgroundWorkItem) => {
    if (item.kind !== "terminal" || !identity || !item.processId) return;
    const expected = identity;
    const snapshot = await invoke<BackgroundProcessSnapshot>("agent_runtime_stop_background_process", {
      tabId,
      request: { ...expected, threadId: item.threadId, processId: item.processId },
    });
    acceptSnapshot(snapshot, expected);
  }, [acceptSnapshot, identity, tabId]);

  const terminals = sameAuthority(loaded.identity, identity) ? loaded.processes : [];
  const items = composerBackgroundWorkItems(messages, terminals, Date.now(), { subagentsVisible, rootThreadId: identity?.rootThreadId, authority: identity ?? undefined });
  return { items, error, stop };
}
