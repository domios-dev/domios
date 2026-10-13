export type PluginUiHostState =
  | { status: "loading" }
  | { status: "ready" }
  | { status: "empty"; reason: string }
  | { status: "error"; reason: string };

export type PluginUiHostEvent =
  | { type: "load" }
  | { type: "ready" }
  | { type: "empty"; reason: string }
  | { type: "error"; reason: string };

export function pluginUiHostTransition(state: PluginUiHostState, event: PluginUiHostEvent): PluginUiHostState {
  if (event.type === "load") return { status: "loading" };
  if (state.status === "error") return state;
  if (event.type === "ready") return { status: "ready" };
  return { status: event.type, reason: event.reason };
}

export function pluginUiDocumentState(html: string | undefined, error: string | null, runtime: PluginUiHostState): PluginUiHostState {
  if (error) return { status: "error", reason: error };
  if (html === undefined) return { status: "loading" };
  if (!html.trim()) return { status: "empty", reason: "The plugin returned an empty UI document." };
  return runtime;
}

export function pluginUiRuntimeEvent(value: unknown): PluginUiHostEvent | null {
  if (!value || typeof value !== "object") return null;
  const event = value as Record<string, unknown>;
  if (event.status === "ready") return { type: "ready" };
  if ((event.status === "error" || event.status === "empty") && typeof event.reason === "string" && event.reason.trim()) {
    return { type: event.status, reason: event.reason.slice(0, 2000) };
  }
  return null;
}
