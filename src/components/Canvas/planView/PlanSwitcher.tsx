import type { ChangeEvent } from "react";

const SEGMENTED_MAX = 3;

export interface PlanSwitcherOption {
  id: string;
  title: string;
}

export interface PlanSwitcherProps {
  /** Pre-ordered by live activity freshness (freshest first). */
  plans: readonly PlanSwitcherOption[];
  selectedId: string | null;
  onSelect: (planId: string) => void;
  label?: string;
}

/** Shared single-plan switcher for Board (and later List): segmented ≤3, dropdown beyond. */
export function PlanSwitcher({ plans, selectedId, onSelect, label = "Plan" }: PlanSwitcherProps) {
  if (plans.length <= 1) return null;

  const onChange = (e: ChangeEvent<HTMLSelectElement>) => onSelect(e.target.value);

  return (
    <div data-testid="plan-switcher" className="plan-board-switcher" role="group" aria-label="Select plan">
      <span className="plan-board-switcher-label">{label}</span>
      {plans.length <= SEGMENTED_MAX ? (
        <div className="plan-board-switcher-seg">
          {plans.map((plan) => {
            const active = plan.id === selectedId;
            return (
              <button
                key={plan.id}
                type="button"
                data-testid="plan-switcher-option"
                data-plan-id={plan.id}
                data-active={active ? "true" : "false"}
                aria-pressed={active}
                className={`plan-board-switcher-btn${active ? " plan-board-switcher-btn-active" : ""}`}
                title={plan.title}
                onClick={() => onSelect(plan.id)}
              >
                {plan.title}
              </button>
            );
          })}
        </div>
      ) : (
        <select
          data-testid="plan-switcher-select"
          className="plan-board-switcher-select"
          value={selectedId ?? ""}
          onChange={onChange}
        >
          {plans.map((plan) => (
            <option key={plan.id} value={plan.id}>
              {plan.title}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
