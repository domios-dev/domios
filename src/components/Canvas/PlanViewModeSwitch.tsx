import { useLayoutEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { planViewModeLabel, type ActivePlanViewMode, type PlanViewMode } from "./planViewMode";
import { usePresence, type PresenceMotion } from "./usePresence";

/** Board stands alone; the plan views share a group that needs an active plan. */
const MODE_GROUPS: ReadonlyArray<{ key: "board" | "plan"; modes: readonly ActivePlanViewMode[] }> = [
  { key: "board", modes: ["board"] },
  { key: "plan", modes: ["spec", "plan", "tasks"] },
];

const TEMPORARY_TAB_MOTION: PresenceMotion = { enter: "ct-segmented-donut-grow", exit: "ct-plan-view-segment-shrink" };

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
  const measuredValue = useRef<string | null>(null);
  const temporary = usePresence<TemporaryModeTab, HTMLDivElement>(temporaryTab ?? null, TEMPORARY_TAB_MOTION);
  const temporaryItem = temporary.item;
  const temporaryClosing = temporary.state === "closing";

  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    // The first placement lands without motion; every later selection glides, whoever changed it.
    if (measuredValue.current !== null && measuredValue.current !== value) setGlide(true);
    measuredValue.current = value;
    const measure = () => {
      const active = root.querySelector<HTMLElement>(`[role="tab"][data-value="${value}"]`);
      if (active) setThumb({ left: active.offsetLeft, width: active.offsetWidth });
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, [value, planModesEnabled, temporaryItem?.label]);

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
                onClick={() => onChange(mode)}
              >
                {planViewModeLabel(mode)}
              </button>
            ))}
          </div>
        );
      })}
      {temporaryItem && (
        <div
          ref={temporary.ref}
          data-group="temporary"
          data-state={temporary.state ?? undefined}
          data-testid="plan-view-temporary-group"
          inert={temporaryClosing || undefined}
          className="plan-view-modes__group plan-view-modes__group--temporary"
        >
          <button
            type="button"
            role="tab"
            id={temporaryClosing ? undefined : temporaryItem.tabId}
            aria-controls={temporaryClosing ? undefined : temporaryItem.panelId}
            data-value={temporaryItem.value}
            data-testid="plan-view-temporary-tab"
            aria-selected={!temporaryClosing && temporaryItem.value === value}
            className="plan-view-modes__option plan-view-modes__option--temporary"
            title={temporaryItem.label}
          >
            {temporaryItem.label}
          </button>
          <button
            type="button"
            data-testid="plan-view-temporary-tab-close"
            aria-label={temporaryItem.closeLabel}
            title={`${temporaryItem.closeLabel} (Esc)`}
            className="plan-view-modes__close"
            onClick={temporaryItem.onClose}
          >
            <X size={11} aria-hidden />
          </button>
        </div>
      )}
    </div>
  );
}
