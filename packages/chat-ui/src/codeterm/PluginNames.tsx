import { createContext, useContext, type ReactNode } from "react";
import type { PluginNameResolver } from "./pluginAction";

const PluginNameContext = createContext<PluginNameResolver | undefined>(undefined);

/** Supplies plugin display names to every chat row below it; rows fall back to the id when absent. */
export function PluginNameProvider({ resolve, children }: { resolve: PluginNameResolver; children: ReactNode }) {
  return <PluginNameContext.Provider value={resolve}>{children}</PluginNameContext.Provider>;
}

export function usePluginNameResolver(): PluginNameResolver | undefined {
  return useContext(PluginNameContext);
}
