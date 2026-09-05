import type { ModelInfo } from "../../lib/types";

export function ModelSuggestionPicker({
  models,
  highlighted,
  defaultModel,
  emptyMessage,
  onHighlight,
  onPick,
}: {
  models: ModelInfo[];
  highlighted: number;
  defaultModel?: string;
  emptyMessage?: string;
  onHighlight: (index: number) => void;
  onPick: (model: ModelInfo) => void;
}) {
  if (models.length === 0 && !emptyMessage) return null;
  return (
    <div
      role={models.length === 0 ? "status" : "listbox"}
      aria-label="Model suggestions"
      className="absolute bottom-full left-0 right-0 mb-1 max-h-56 overflow-y-auto rounded"
      style={{
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
        border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
        zIndex: 10,
      }}
    >
      {models.length === 0 ? (
        <p className="px-2 py-1.5 text-xs text-[var(--ct-muted-fg)]">{emptyMessage}</p>
      ) : models.map((model, i) => {
        const selected = i === highlighted;
        const isDefault = defaultModel === model.id;
        const unavailable = model.available === false;
        return (
          <button
            key={model.id}
            type="button"
            disabled={unavailable}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onHighlight(i)}
            onClick={() => onPick(model)}
            role="option"
            aria-selected={selected}
            aria-disabled={unavailable}
            title={unavailable ? `${model.display_name} is not available yet` : (model.description ?? model.id)}
            className="grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-2 px-2 py-1.5 text-left text-xs"
            style={{
              background: selected && !unavailable
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 20%, transparent)"
                : "transparent",
              color: selected && !unavailable ? "var(--ct-fg, #c9d1d9)" : "var(--ct-muted-fg, #8b949e)",
              border: "none",
              cursor: unavailable ? "not-allowed" : "pointer",
              opacity: unavailable ? 0.5 : 1,
            }}
          >
            <span className="min-w-0">
              <span className="block truncate font-medium">
                {model.display_name}
                {unavailable ? " (unavailable)" : ""}
              </span>
              <span className="block truncate font-mono opacity-65">{model.id}</span>
            </span>
            {isDefault && (
              <span
                className="rounded-full border px-1.5 py-0.5 text-[9px] uppercase tracking-normal"
                style={{
                  borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 35%, transparent)",
                  color: "var(--ct-accent, #58a6ff)",
                }}
              >
                default
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
