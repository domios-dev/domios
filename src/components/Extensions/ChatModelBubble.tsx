/**
 * Composer-row model picker for chatBackend shell panes. A shell resolves identity
 * from `backing.plugin_id` (not the agent registry); picking a model calls
 * `chatbackend_switch_model` and reflects the refreshed session info.
 * Generalized — any chatBackend shell with `available_models`, no hardcoded id.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronUp } from "lucide-react";
import type { ModelInfo } from "../../lib/types";
import { getChatModelBubbleApi } from "./chatModelBubbleApi";
import { ConfirmDialog } from "./ConfirmDialog";

export function ChatModelBubble({ pluginId, sessionId }: { pluginId: string; sessionId: string }) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [currentModel, setCurrentModel] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<{ model: ModelInfo; message: string } | null>(null);

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

  const doSwitch = useCallback(
    async (model: ModelInfo) => {
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

  const choose = useCallback(
    async (model: ModelInfo) => {
      setOpen(false);
      if (model.id === currentModel) return;
      // The plugin decides whether the switch needs confirmation; absent hook / rejection → switch directly.
      let desc = null;
      try {
        desc = (await getChatModelBubbleApi().describeModelSwitch?.(pluginId, sessionId, model.id)) ?? null;
      } catch {
        desc = null;
      }
      if (desc?.needsConfirm) {
        setPending({ model, message: desc.message });
        return;
      }
      await doSwitch(model);
    },
    [pluginId, sessionId, currentModel, doSwitch],
  );

  const confirmPending = useCallback(() => {
    const model = pending?.model;
    setPending(null);
    if (model) void doSwitch(model);
  }, [pending, doSwitch]);

  // Generalized gate: only shells that publish switchable models get the bubble.
  if (models.length === 0) return null;

  return (
    <div
      className="relative inline-flex"
      style={{
        zIndex: 20,
      }}
    >
      <button
        type="button"
        data-testid="chat-model-bubble"
        className="ct-chat-model-bubble inline-flex h-7 items-center gap-1 rounded-full border px-2 text-xs font-mono hover:brightness-110"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((v) => !v)}
        disabled={busy}
        title="Switch model"
        style={{
          background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
          borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
          color: "var(--ct-muted-fg)",
          opacity: busy ? 0.6 : 1,
        }}
      >
        <span>{label ?? "Select model"}</span>
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
      {pending && (
        <ConfirmDialog
          testId="chat-model-switch-confirm"
          title="Switch model?"
          message={pending.message}
          confirmLabel="Switch"
          onConfirm={confirmPending}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}
