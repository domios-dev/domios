import React, { createContext, useContext, useMemo } from "react";
import type { LiveBindingSource } from "./liveBinding";
import type { ConnectBlockState } from "./connectBlockModel";
import type { PluginBlockState, PluginLifecycleKind } from "./pluginBlockModel";
import type { SettingBlockState, SettingValue } from "./settingBlockModel";

/** Host services the plugin block reads and acts through; every mutation runs only from a user click. */
export interface PluginBlockHost {
  binding: LiveBindingSource<PluginBlockState>;
  run(pluginId: string, kind: PluginLifecycleKind): Promise<void>;
  openView(pluginId: string): void;
  openInPlugins(pluginId: string): void;
  renderIcon?(pluginId: string, size: number): React.ReactNode;
}

/** Messenger bridge services; connect and disconnect run only from a user click. */
export interface ConnectBlockHost {
  binding: LiveBindingSource<ConnectBlockState>;
  /** Saves a given token to the secret store first, then starts the bridge with it. */
  connect(bridgeId: string, token?: string): Promise<void>;
  disconnect(bridgeId: string): Promise<void>;
  openSettings(bridgeId: string): void;
}

/** One config value through the settings schema; `write` is the config set path and runs only from a user click. */
export interface SettingBlockHost {
  binding: LiveBindingSource<SettingBlockState>;
  write(key: string, value: SettingValue): Promise<void>;
  openSettings?(key: string): void;
}

/** Scope-wide services for chat blocks. Absent fields hide the matching control instead of faking it. */
export interface ChatBlockHost {
  askAbout?(text: string): void;
  pin?(source: string): void;
  pinned?: LiveBindingSource<boolean>;
  plugins?: PluginBlockHost;
  connect?: ConnectBlockHost;
  settings?: SettingBlockHost;
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
