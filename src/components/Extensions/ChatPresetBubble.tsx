/**
 * Composer-row preset bubble for chatBackend chat panes (R5b follow-up).
 *
 * Mirrors {@link ChatModelBubble}: a compact picker in the chat chrome that
 * lists the backend's user-defined presets ({@link presetManagerApi}) and, on
 * pick, persists the choice as `activePreset` (so the plugin's `listPresets()`
 * and its bound system prompt are read back) AND — when a live `sessionId` is
 * supplied — applies the preset's bound model to that session via the shared
 * `switch_model` seam. Without a `sessionId` it only persists (management-only),
 * which is how {@link PresetManager} behaves in the extensions panel.
 * Generalized — any chatBackend with presets gets it; no hardcoded provider id.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronUp } from "lucide-react";
import { getPresetManagerApi, type Preset, type PresetState } from "./presetManagerApi";

export function ChatPresetBubble({ pluginId, sessionId }: { pluginId: string; sessionId?: string }) {
  const [state, setState] = useState<PresetState>({ presets: [], activePreset: null });
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    getPresetManagerApi()
      .load(pluginId)
      .then((loaded) => {
        if (alive) setState(loaded);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [pluginId]);

  const label = useMemo(() => {
    const active = state.presets.find((p) => p.id === state.activePreset);
    return active?.name || null;
  }, [state]);

  const choose = useCallback(
    async (preset: Preset) => {
      setOpen(false);
      setBusy(true);
      const next: PresetState = { ...state, activePreset: preset.id };
      setState(next);
      try {
        const api = getPresetManagerApi();
        await api.save(pluginId, next);
        if (sessionId && preset.model) {
          await api.applyToSession(pluginId, sessionId, preset.model);
        }
      } catch {
        // Leave the persisted choice as-is; honest over rolling back a partial apply.
      } finally {
        setBusy(false);
      }
    },
    [pluginId, sessionId, state],
  );

  // Generalized gate: only backends that publish presets get the bubble.
  if (state.presets.length === 0) return null;

  return (
    <div className="relative inline-flex" style={{ zIndex: 20 }}>
      <button
        type="button"
        data-testid="chat-preset-bubble"
        className="ct-chat-preset-bubble inline-flex h-7 items-center gap-1 rounded-full border px-2 text-xs font-mono hover:brightness-110"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((v) => !v)}
        disabled={busy}
        title={sessionId ? "Apply a preset to this session" : "Choose the active preset"}
        style={{
          background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
          borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
          color: "var(--ct-muted-fg)",
          opacity: busy ? 0.6 : 1,
        }}
      >
        <span>{label ?? "Preset"}</span>
        <ChevronUp size={11} className="opacity-70" />
      </button>
      {open && (
        <div
          className="absolute bottom-full left-0 mb-1 grid gap-0.5 rounded-md border p-1 shadow-lg"
          style={{
            minWidth: 180,
            maxHeight: 260,
            overflowY: "auto",
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, black 6%)",
            borderColor: "color-mix(in srgb, var(--ct-accent, #5b8cff) 35%, transparent)",
          }}
        >
          {state.presets.map((preset) => {
            const selected = preset.id === state.activePreset;
            return (
              <button
                key={preset.id}
                type="button"
                data-testid="chat-preset-option"
                data-preset-id={preset.id}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => void choose(preset)}
                className="w-full grid grid-cols-[0.75rem_minmax(0,1fr)] gap-2 rounded px-2 py-1 text-left text-xs"
                style={{
                  background: selected
                    ? "color-mix(in srgb, var(--ct-accent, #5b8cff) 18%, transparent)"
                    : "transparent",
                  color: selected ? "var(--ct-fg)" : "var(--ct-muted-fg)",
                  border: "none",
                  cursor: "pointer",
                }}
              >
                <span className="text-center font-mono">{selected ? ">" : ""}</span>
                <span className="min-w-0">
                  <span className="block truncate font-medium">{preset.name || "Untitled"}</span>
                  {preset.model && (
                    <span className="block truncate opacity-60 font-mono">{preset.model}</span>
                  )}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
