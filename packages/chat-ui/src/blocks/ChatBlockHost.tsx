import React, { createContext, useContext, useMemo } from "react";
import type { LiveBindingSource } from "./liveBinding";
import type { PluginBlockState, PluginLifecycleKind } from "./pluginBlockModel";

/** Host services the plugin block reads and acts through; every mutation runs only from a user click. */
export interface PluginBlockHost {
  binding: LiveBindingSource<PluginBlockState>;
  run(pluginId: string, kind: PluginLifecycleKind): Promise<void>;
  openView(pluginId: string): void;
  openInPlugins(pluginId: string): void;
  renderIcon?(pluginId: string, size: number): React.ReactNode;
}

/** Scope-wide services for chat blocks. Absent fields hide the matching control instead of faking it. */
export interface ChatBlockHost {
  askAbout?(text: string): void;
  pin?(source: string): void;
  pinned?: LiveBindingSource<boolean>;
  plugins?: PluginBlockHost;
}

const ChatBlockHostContext = createContext<ChatBlockHost>({});

/** Nested providers merge: a tab scope adds Ask and Pin on top of the app-wide data services. */
export function ChatBlockHostProvider({ value, children }: { value: ChatBlockHost; children: React.ReactNode }) {
  const parent = useContext(ChatBlockHostContext);
  const merged = useMemo(
    () => ({ ...parent, ...Object.fromEntries(Object.entries(value).filter(([, service]) => service !== undefined)) }),
    [parent, value],
  );
  return <ChatBlockHostContext.Provider value={merged}>{children}</ChatBlockHostContext.Provider>;
}

export function useChatBlockHost(): ChatBlockHost {
  return useContext(ChatBlockHostContext);
}
