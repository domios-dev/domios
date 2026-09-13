import { useLayoutEffect, useRef, useState } from "react";
import { planViewModeLabel, type ActivePlanViewMode, type PlanViewMode } from "./planViewMode";

/** Board stands alone; the plan views share a group that needs an active plan. */
const MODE_GROUPS: ReadonlyArray<{ key: "board" | "plan"; modes: readonly ActivePlanViewMode[] }> = [
  { key: "board", modes: ["board"] },
  { key: "plan", modes: ["spec", "plan", "tasks"] },
];

interface ThumbBox {
  left: number;
  width: number;
}

/** One tablist drawn as two capsules with a single thumb that glides between them. */
export function PlanViewModeSwitch({
  value,
  onChange,
  planModesEnabled = true,
}: {
  value: PlanViewMode;
  onChange: (mode: PlanViewMode) => void;
  planModesEnabled?: boolean;
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [thumb, setThumb] = useState<ThumbBox | null>(null);
  const [glide, setGlide] = useState(false);

  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const measure = () => {
      const active = root.querySelector<HTMLElement>(`[data-value="${value}"]`);
      if (active) setThumb({ left: active.offsetLeft, width: active.offsetWidth });
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, [value, planModesEnabled]);

  return (
    <div
      ref={rootRef}
      role="tablist"
      aria-label="Plan view"
      data-testid="plan-view-mode-switch"
      className="plan-view-modes"
    >
      <span
        aria-hidden
        data-testid="plan-view-mode-thumb"
        className={`plan-view-modes__thumb${glide ? " plan-view-modes__thumb--glide" : ""}`}
        style={thumb ? { transform: `translateX(${thumb.left}px)`, width: thumb.width } : { opacity: 0 }}
      />
      {MODE_GROUPS.map((group) => {
        const disabled = group.key === "plan" && !planModesEnabled;
        return (
          <div
            key={group.key}
            data-group={group.key}
            data-disabled={disabled ? "true" : undefined}
            className="plan-view-modes__group"
          >
            {group.modes.map((mode) => (
              <button
                key={mode}
                type="button"
                role="tab"
                data-value={mode}
                aria-selected={mode === value}
                disabled={disabled}
                className="plan-view-modes__option"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  setGlide(true);
                  onChange(mode);
                }}
              >
                {planViewModeLabel(mode)}
              </button>
            ))}
          </div>
        );
      })}
    </div>
  );
}
