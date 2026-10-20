import { InlineErrorNotice } from "../shared/ErrorNotice";
import { removedChatPluginId } from "./removedChatPluginError";

export function RemovedChatPlugin({ error }: { error: string | null }) {
  const pluginId = removedChatPluginId(error);
  if (pluginId === null) return null;
  return (
    <div aria-label="Plugin removed"><InlineErrorNotice error={{ code: "plugin_removed", raw: error ?? pluginId }} /></div>
  );
}
