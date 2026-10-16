import { createContext, useContext, type ReactNode } from "react";

const InspectionContext = createContext<((threadId: string) => void) | undefined>(undefined);

export function AgentInspectionProvider({ onInspect, children }: { onInspect?: (threadId: string) => void; children: ReactNode }) {
  return <InspectionContext.Provider value={onInspect}>{children}</InspectionContext.Provider>;
}

export function useAgentInspection() { return useContext(InspectionContext); }
