/**
 * Top-of-pane model bubble for chatBackend SHELL panes (CB-P3).
 *
 * Mirrors the agent model-picker UX, but a shell resolves its identity from the
 * plugin (`backing.plugin_id`), not the agent registry: it shows the live model
 * (CB-P1 `chatbackend_session_info`) and a dropdown of the plugin's switchable
 * models (CB-P1 snapshot). Picking one calls `chatbackend_switch_model` and the
 * bubble reflects the refreshed session info. Generalized — any chatBackend
 * shell with `available_models` gets it; no hardcoded provider id.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronUp } from "lucide-react";
import type { ModelInfo } from "../../lib/types";
import { getChatModelBubbleApi } from "./chatModelBubbleApi";

export function ChatModelBubble({ pluginId, sessionId }: { pluginId: string; sessionId: string }) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [currentModel, setCurrentModel] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    const api = getChatModelBubbleApi();
    Promise.all([api.availableModels(pluginId), api.sessionInfo(pluginId, sessionId)])
      .then(([available, info]) => {
        if (!alive) return;
        setModels(available);
        setCurrentModel(info?.model ?? null);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [pluginId, sessionId]);

  const label = useMemo(() => {
    if (!currentModel) return null;
    return models.find((m) => m.id === currentModel)?.display_name ?? currentModel;
  }, [currentModel, models]);

  const choose = useCallback(
    async (model: ModelInfo) => {
      setOpen(false);
      setBusy(true);
      try {
        const info = await getChatModelBubbleApi().switchModel(pluginId, sessionId, model.id);
        setCurrentModel(info?.model ?? model.id);
      } catch {
        // Leave the displayed model unchanged on failure — honest over guessing.
      } finally {
        setBusy(false);
      }
    },
    [pluginId, sessionId],
  );

  // Generalized gate: only shells that publish switchable models get the bubble.
  if (models.length === 0) return null;

  return (
    <div
      style={{
        position: "absolute",
        top: "calc(6px + var(--ct-safe-top, 0px))",
        left: 8,
        zIndex: 6,
        pointerEvents: "auto",
      }}
    >
      <button
        type="button"
        data-testid="chat-model-bubble"
        onClick={() => setOpen((v) => !v)}
        disabled={busy}
        title="Switch model"
        style={{
          display: "flex",
          alignItems: "center",
          gap: 5,
          height: 18,
          fontSize: 10.5,
          fontWeight: 560,
          padding: "0 8px",
          borderRadius: 999,
          color: "var(--ct-accent, #5b8cff)",
          background: "color-mix(in srgb, var(--ct-bg-elev, #1e1e2e) 80%, transparent)",
          border: "1px solid color-mix(in srgb, var(--ct-accent, #5b8cff) 32%, transparent)",
          backdropFilter: "blur(10px) saturate(140%)",
          cursor: busy ? "default" : "pointer",
          whiteSpace: "nowrap",
          opacity: busy ? 0.6 : 1,
        }}
      >
        <span>{label ?? "Select model"}</span>
        <ChevronUp size={11} className="opacity-70" />
      </button>
      {open && (
        <div
          className="mt-1 grid gap-0.5 rounded-md border p-1 shadow-lg"
          style={{
            position: "absolute",
            top: "100%",
            left: 0,
            minWidth: 180,
            maxHeight: 260,
            overflowY: "auto",
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, black 6%)",
            borderColor: "color-mix(in srgb, var(--ct-accent, #5b8cff) 35%, transparent)",
          }}
        >
          {models.map((model) => {
            const selected = model.id === currentModel;
            return (
              <button
                key={model.id}
                type="button"
                data-testid="chat-model-option"
                data-model-id={model.id}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => void choose(model)}
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
                  <span className="block truncate font-medium">{model.display_name}</span>
                  <span className="block truncate opacity-60 font-mono">{model.id}</span>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
