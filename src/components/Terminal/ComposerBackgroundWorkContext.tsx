import { createContext, useContext, type ReactNode } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

const ComposerBackgroundWorkContext = createContext<readonly NormalizedChatMessage[]>([]);

export function ComposerBackgroundWorkProvider({ messages, children }: { messages: readonly NormalizedChatMessage[]; children: ReactNode }) {
  return <ComposerBackgroundWorkContext.Provider value={messages}>{children}</ComposerBackgroundWorkContext.Provider>;
}

export function useComposerBackgroundWorkMessages(): readonly NormalizedChatMessage[] {
  return useContext(ComposerBackgroundWorkContext);
}
