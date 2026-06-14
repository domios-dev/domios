import type { ModelInfo } from "../../lib/types";

export function ModelSuggestionPicker({
  models,
  highlighted,
  defaultModel,
  onHighlight,
  onPick,
}: {
  models: ModelInfo[];
  highlighted: number;
  defaultModel?: string;
  onHighlight: (index: number) => void;
  onPick: (model: ModelInfo) => void;
}) {
  if (models.length === 0) return null;
  return (
    <div
      className="absolute bottom-full left-0 right-0 mb-1 max-h-56 overflow-y-auto rounded"
      style={{
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
        border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
        zIndex: 10,
      }}
    >
      {models.map((model, i) => {
        const selected = i === highlighted;
        const isDefault = defaultModel === model.id;
        return (
          <button
            key={model.id}
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onHighlight(i)}
            onClick={() => onPick(model)}
            title={model.description ?? model.id}
            className="grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-2 px-2 py-1.5 text-left text-xs"
            style={{
              background: selected
                ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 20%, transparent)"
                : "transparent",
              color: selected ? "var(--ct-fg, #c9d1d9)" : "var(--ct-muted-fg, #8b949e)",
              border: "none",
              cursor: "pointer",
            }}
          >
            <span className="min-w-0">
              <span className="block truncate font-medium">{model.display_name}</span>
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
