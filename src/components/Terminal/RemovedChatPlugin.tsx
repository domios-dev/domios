import { removedChatPluginId } from "./removedChatPluginError";

export function RemovedChatPlugin({ error }: { error: string | null }) {
  const pluginId = removedChatPluginId(error);
  if (pluginId === null) return null;
  return (
    <div role="alert" aria-label="Plugin removed" className="p-4">
      <p>Plugin removed: {pluginId}</p>
      <p>Open a new session with an installed provider.</p>
    </div>
  );
}
