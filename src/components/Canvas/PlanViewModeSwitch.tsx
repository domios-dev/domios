import { useLayoutEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { planViewModeLabel, type ActivePlanViewMode, type PlanViewMode } from "./planViewMode";

/** Board stands alone; the plan views share a group that needs an active plan. */
const MODE_GROUPS: ReadonlyArray<{ key: "board" | "plan"; modes: readonly ActivePlanViewMode[] }> = [
  { key: "board", modes: ["board"] },
  { key: "plan", modes: ["spec", "plan", "tasks"] },
];

/** A closable tab that exists only while it is selected; choosing any mode removes it. */
export interface TemporaryModeTab {
  value: string;
  label: string;
  closeLabel: string;
  tabId: string;
  panelId: string;
  onClose: () => void;
}

interface ThumbBox {
  left: number;
  width: number;
}

/** One tablist drawn as capsules with a single thumb that glides between them. */
export function PlanViewModeSwitch({
  value,
  onChange,
  planModesEnabled = true,
  temporaryTab,
}: {
  value: PlanViewMode | string;
  onChange: (mode: PlanViewMode) => void;
  planModesEnabled?: boolean;
  temporaryTab?: TemporaryModeTab | null;
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [thumb, setThumb] = useState<ThumbBox | null>(null);
  const [glide, setGlide] = useState(false);

  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const measure = () => {
      const active = root.querySelector<HTMLElement>(`[role="tab"][data-value="${value}"]`);
      if (active) setThumb({ left: active.offsetLeft, width: active.offsetWidth });
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, [value, planModesEnabled, temporaryTab?.label]);

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
      {temporaryTab && (
        <div data-group="temporary" className="plan-view-modes__group plan-view-modes__group--temporary">
          <button
            type="button"
            role="tab"
            id={temporaryTab.tabId}
            aria-controls={temporaryTab.panelId}
            data-value={temporaryTab.value}
            data-testid="plan-view-temporary-tab"
            aria-selected={temporaryTab.value === value}
            className="plan-view-modes__option plan-view-modes__option--temporary"
            title={temporaryTab.label}
          >
            {temporaryTab.label}
          </button>
          <button
            type="button"
            data-testid="plan-view-temporary-tab-close"
            aria-label={temporaryTab.closeLabel}
            title={`${temporaryTab.closeLabel} (Esc)`}
            className="plan-view-modes__close"
            onClick={() => {
              setGlide(true);
              temporaryTab.onClose();
            }}
          >
            <X size={11} aria-hidden />
          </button>
        </div>
      )}
    </div>
  );
}
