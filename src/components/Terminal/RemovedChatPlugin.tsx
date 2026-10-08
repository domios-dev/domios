import { removedChatPluginId } from "./removedChatPluginError";

export function RemovedChatPlugin({ error }: { error: string | null }) {
  const pluginId = removedChatPluginId(error);
  if (pluginId === null) return null;
  return (
    <div role="alert" aria-label="Plugin removed" className="mx-auto w-full max-w-sm p-4 text-center">
      <div className="text-sm font-semibold" style={{ color: "var(--ct-fg)" }}>
        This chat's plugin was removed
      </div>
      <div className="mt-1 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        Open a new session with an installed provider.
      </div>
      <div className="mt-2 font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }} title="Removed plugin id">
        {pluginId}
      </div>
    </div>
  );
}
