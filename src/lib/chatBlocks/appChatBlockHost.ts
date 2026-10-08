import type { ChatBlockHost } from "@codeterm/chat-ui";
import {
  ensurePluginSnapshot,
  getPluginSnapshotState,
  runPluginSnapshotAction,
  subscribePluginSnapshotStore,
} from "../pluginSnapshotStore";
import { createPluginBlockHost } from "./pluginBlockHost";

/** App-wide data services for chat blocks; tab scopes add Ask and Pin on top. */
export const appChatBlockHost: ChatBlockHost = {
  plugins: createPluginBlockHost({
    get: getPluginSnapshotState,
    subscribe: subscribePluginSnapshotStore,
    ensure: () => ensurePluginSnapshot({ background: true }),
    runAction: runPluginSnapshotAction,
  }),
};
