import { useCallback, type KeyboardEvent } from "react";
import { List } from "lucide-react";

export interface MeshListSuggestionBannerProps {
  count: number;
  onSwitchToList: () => void;
  onKeepCanvas: () => void;
}

export function MeshListSuggestionBanner({
  count,
  onSwitchToList,
  onKeepCanvas,
}: MeshListSuggestionBannerProps) {
  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onKeepCanvas();
    },
    [onKeepCanvas],
  );

  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="mesh-list-suggestion"
      onKeyDown={handleKeyDown}
      onMouseDown={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
      className="ct-mesh-capsule"
    >
      <div className="ct-mesh-capsule__copy">
        <List size={14} className="shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
        <span className="min-w-0">
          <strong className="font-semibold text-[var(--ct-fg)]">{count} machines now.</strong>{" "}
          <span className="text-[var(--ct-muted-fg)]">Switch to a list?</span>
        </span>
      </div>
      <div className="ct-mesh-capsule__actions">
        <button
          type="button"
          onClick={onKeepCanvas}
          className="ct-mesh-capsule__btn ct-mesh-capsule__btn--ghost"
        >
          Keep canvas
        </button>
        <button
          type="button"
          onClick={onSwitchToList}
          className="ct-mesh-capsule__btn ct-mesh-capsule__btn--primary"
        >
          Switch to list
        </button>
      </div>
    </div>
  );
}
