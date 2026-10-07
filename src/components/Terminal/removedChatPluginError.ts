export function removedChatPluginId(error: string | null): string | null {
  if (!error) return null;
  try {
    const value: unknown = JSON.parse(error);
    if (value === null || typeof value !== "object") return null;
    const failure = value as Record<string, unknown>;
    return failure.code === "chat_backend_plugin_removed" && typeof failure.plugin_id === "string"
      ? failure.plugin_id : null;
  } catch {
    return null;
  }
}
