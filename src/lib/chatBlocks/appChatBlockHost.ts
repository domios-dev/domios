import type { ChatBlockHost } from "@codeterm/chat-ui";
import { selectExtension } from "../../components/Titlebar/extensionsTrayLogic";
import { fetchAgentProviders } from "../../hooks/useAgentProviders";
import { listAgentDescriptors, setAgentDescriptors, subscribeAgentDescriptors } from "../agentDescriptorCache";
import { appConfigSource } from "../appConfigSource";
import { telegramConnectInfo, type TelegramBridgeRaw } from "../messengers/telegramBridge";
import { telegramBridgeStore } from "../messengers/telegramBridgeStore";
import {
  ensurePluginSnapshot,
  getPluginSnapshotState,
  runPluginSnapshotAction,
  subscribePluginSnapshotStore,
} from "../pluginSnapshotStore";
import { createConnectBlockHost, type MessengerBridge } from "./connectBlockHost";
import { createPluginBlockHost } from "./pluginBlockHost";
import { createSettingBlockHost } from "./settingBlockHost";
import { renderChatCardMenu } from "../../components/Terminal/ChatCardMenu";

function openPanel(panel: string) {
  window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel } }));
}

const telegram: MessengerBridge<TelegramBridgeRaw> = { store: telegramBridgeStore, describe: telegramConnectInfo };

/** App-wide data services for chat blocks; tab scopes add Ask and Pin on top. */
export const appChatBlockHost: ChatBlockHost = {
  renderMenu: renderChatCardMenu,
  plugins: createPluginBlockHost({
    get: getPluginSnapshotState,
    subscribe: subscribePluginSnapshotStore,
    ensure: () => ensurePluginSnapshot({ background: true }),
    runAction: runPluginSnapshotAction,
  }),
  connect: createConnectBlockHost({ telegram }, () => openPanel("settings:messengers")),
  settings: createSettingBlockHost({
    config: appConfigSource,
    plugins: {
      get: getPluginSnapshotState,
      subscribe: subscribePluginSnapshotStore,
      ensure: () => ensurePluginSnapshot({ background: true }),
    },
    agents: {
      list: listAgentDescriptors,
      subscribe: subscribeAgentDescriptors,
      ensure: async () => setAgentDescriptors(await fetchAgentProviders()),
    },
    openPlugin: (pluginId) => selectExtension(pluginId, () => openPanel("extensions")),
  }),
};
