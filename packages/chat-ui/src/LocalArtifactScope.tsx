import { createContext, useContext, type ReactNode } from "react";

const ArtifactTabContext = createContext<string | undefined>(undefined);

export function LocalArtifactScope({ tabId, children }: { tabId: string; children: ReactNode }) {
  return <ArtifactTabContext.Provider value={tabId}>{children}</ArtifactTabContext.Provider>;
}

export function useLocalArtifactTabId(): string | undefined {
  return useContext(ArtifactTabContext);
}
