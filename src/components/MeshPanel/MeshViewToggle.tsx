import { useCallback, type KeyboardEvent } from "react";
import { List, Network } from "lucide-react";
import type { MeshInventoryViewMode } from "./meshViewModeModel";

export interface MeshViewToggleProps {
  value: MeshInventoryViewMode;
  onChange: (next: MeshInventoryViewMode) => void;
  iconOnly?: boolean;
  touch?: boolean;
}

const OPTIONS: ReadonlyArray<{ value: MeshInventoryViewMode; label: string; Icon: typeof Network }> = [
  { value: "canvas", label: "Canvas", Icon: Network },
  { value: "list", label: "List", Icon: List },
];

export function MeshViewToggle({
  value,
  onChange,
  iconOnly = false,
  touch = false,
}: MeshViewToggleProps) {
  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight" && event.key !== "Home" && event.key !== "End") {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const next: MeshInventoryViewMode =
        event.key === "Home"
          ? "canvas"
          : event.key === "End"
            ? "list"
            : value === "canvas"
              ? "list"
              : "canvas";
      onChange(next);
      const target = event.currentTarget.querySelector<HTMLButtonElement>(`[data-value="${next}"]`);
      target?.focus({ preventScroll: true });
    },
    [onChange, value],
  );

  return (
    <div
      role="radiogroup"
      aria-label="Mesh view"
      data-testid="mesh-inventory-view-toggle"
      onKeyDown={handleKeyDown}
      className={[
        "ct-mesh-view-rail",
        iconOnly ? "ct-mesh-view-rail--icon" : "",
        touch ? "ct-mesh-view-rail--touch" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {OPTIONS.map(({ value: optionValue, label, Icon }) => {
        const checked = value === optionValue;
        return (
          <button
            key={optionValue}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={label}
            title={label}
            tabIndex={checked ? 0 : -1}
            data-value={optionValue}
            data-testid={`mesh-view-toggle-${optionValue}`}
            onClick={() => onChange(optionValue)}
            className={`ct-mesh-view-rail__opt${checked ? " ct-mesh-view-rail__opt--active" : ""}`}
          >
            <Icon size={13} aria-hidden="true" className="shrink-0" />
            {!iconOnly && <span>{label}</span>}
          </button>
        );
      })}
    </div>
  );
}
