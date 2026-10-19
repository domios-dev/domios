import { createContext, useContext, type ReactNode } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { useComposerBackgroundWork } from "./useComposerBackgroundWork";
import { useMuxSelector } from "../../store/multiplexer";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";

type BackgroundWork = ReturnType<typeof useComposerBackgroundWork>;
const ComposerBackgroundWorkContext = createContext<BackgroundWork>({items:[],stop:async()=>{}});

export function ComposerBackgroundWorkProvider({ tabId, messages, children }: { tabId: string; messages: readonly NormalizedChatMessage[]; children: ReactNode }) {
  const providerId = useMuxSelector(state => {
    const tab = state.tabs[tabId];
    return tab?.agent_type ?? (tab?.backing?.kind === "chat_backend" ? tab.backing.plugin_id : null);
  });
  const descriptor = useAgentDescriptor(providerId);
  const work = useComposerBackgroundWork(tabId, descriptor?.agent_runtime?.backgroundProcesses === true, descriptor?.supports_subagent_visibility === true, messages);
  return <ComposerBackgroundWorkContext.Provider value={work}>{children}</ComposerBackgroundWorkContext.Provider>;
}

export function useComposerBackgroundWorkStatus(): BackgroundWork {
  return useContext(ComposerBackgroundWorkContext);
}
