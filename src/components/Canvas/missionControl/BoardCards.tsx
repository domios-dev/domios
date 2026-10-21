import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { AnchoredPopover } from "../../shared/AnchoredPopover";
import type { Plan } from "../types";
import type { ApiConfig } from "../../../lib/types";
import { PlanModeControls } from "../planView/PlanModeControls";
import { PLANNING_STATE_LABEL, planDraftState } from "../planView/boardModel";
import {
  formatCount,
  formatPlanDate,
  planGoal,
  progressReading,
} from "./missionControlBoardModel";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection, ShowAll } from "./BoardPanel";
import { PlanGoal, PlanProgressBar, PlanProgressStats } from "./BoardPlanProgress";
import type { BoardRefs } from "./BoardRefText";
import {
  NeedsYouNotificationRow,
  useNeedsYouNotificationItems,
  type NeedsYouDeepLinks,
} from "../../Notifications/NeedsYouNotificationRow";
import type { BounceSummary, PlanTaskCounts } from "./missionControlState";

export { ModelsCard } from "./BoardModelsCard";
export { SpendCard } from "./BoardSpendCard";

const NEEDS_YOU_PREVIEW = 3;

/** The team's unanswered questions to the owner; empty once every one is answered or cleared. */
export function NeedsYouPanel({
  refs = new Map(),
  deepLinks,
}: {
  refs?: BoardRefs;
  deepLinks?: NeedsYouDeepLinks;
}) {
  const notificationItems = useNeedsYouNotificationItems(deepLinks);
  const [showAll, setShowAll] = useState(false);
  const listId = useId();
  const total = notificationItems.length;
  if (total === 0 || !deepLinks) return null;
  const visibleNotifications = showAll ? notificationItems : notificationItems.slice(0, NEEDS_YOU_PREVIEW);
  const onClear = deepLinks.onClear;
  return (
    <BoardPanel
      testId="mc-board-needs-you"
      title="Needs you"
      meta={onClear ? (
        <button
          type="button"
          data-testid="mc-board-needs-you-clear"
          className="mc-board-segment mc-board-segment-solo"
          onClick={() => onClear(notificationItems.map((item) => item.id))}
        >
          clear
        </button>
      ) : undefined}
    >
      <div className="mc-board-needs">
        <span data-testid="mc-board-needs-you-count" className="mc-board-figure mc-board-warn">
          <AnimatedNumber value={total} format={formatCount} />
        </span>
        <div className="mc-board-needs-body">
          <ul id={listId} className="mc-board-needs-list">
            {visibleNotifications.map((item) => (
              <NeedsYouNotificationRow
                key={item.key}
                item={item}
                refs={refs}
                tabs={deepLinks.tabs}
                onSelectTab={deepLinks.onSelectTab}
              />
            ))}
          </ul>
          <ShowAll
            total={total}
            limit={NEEDS_YOU_PREVIEW}
            open={showAll}
            controls={listId}
            testId="mc-board-needs-you-more"
            onToggle={() => setShowAll((value) => !value)}
          />
        </div>
      </div>
    </BoardPanel>
  );
}

function PreviousPlanList({
  id,
  plans,
  onOpenPlan,
}: {
  id: string;
  plans: readonly Plan[];
  onOpenPlan?: (planId: string) => void;
}) {
  return (
    <ul id={id} className="mc-board-plan-list" role="list">
      {plans.map((previous) => (
        <li key={previous.id}>
          {onOpenPlan ? (
            <button
              type="button"
              data-testid="mc-board-previous-plan"
              className="mc-board-ref"
              onClick={() => onOpenPlan(previous.id)}
            >
              {previous.title}
            </button>
          ) : (
            <span data-testid="mc-board-previous-plan">{previous.title}</span>
          )}
          <span data-testid="mc-board-previous-plan-date" className="mc-board-plan-list-date">
            {formatPlanDate(previous.updated_at)}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** A quiet footer trigger whose menu of previous plans floats above the Board; choosing one opens its detail. */
function PreviousPlansMenu({
  plans,
  onOpenPlan,
}: {
  plans: readonly Plan[];
  onOpenPlan?: (planId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const dismiss = useCallback(() => setOpen(false), []);
  const itemsRef = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    if (!open) return;
    if (open) itemsRef.current[0]?.focus();
  }, [open]);

  const close = () => {
    setOpen(false);
    triggerRef.current?.focus();
  };

  const choose = (planId: string) => {
    close();
    onOpenPlan?.(planId);
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key === "Tab") {
      setOpen(false);
      return;
    }
    const items = itemsRef.current.filter((item): item is HTMLButtonElement => item != null);
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Enter" || event.key === " ") {
      const item = items.find((candidate) => candidate === event.target);
      if (!item) return;
      event.preventDefault();
      item.click();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const step = event.key === "ArrowDown" ? 1 : -1;
    items[(current + step + items.length) % items.length]?.focus();
  };

  return (
    <div data-testid="mc-board-previous-plans" className="mc-board-plan-footer">
      <button
        ref={triggerRef}
        type="button"
        data-testid="mc-board-previous-plans-toggle"
        className="mc-board-more"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((current) => !current)}
      >
        Previous plans <span className="mc-board-plan-count">{formatCount(plans.length)}</span>
      </button>
      <AnchoredPopover open={open} anchor={triggerRef} placement="top-start" onClose={dismiss}>
          <div
            id={menuId}
            role="menu"
            aria-label="Previous plans"
            data-testid="mc-board-previous-plans-menu"
            className="mc-board-layer ct-menu mc-board-plan-menu"
            onKeyDown={onMenuKeyDown}
          >
            {plans.map((previous, index) => (
              <button
                key={previous.id}
                ref={(item) => {
                  itemsRef.current[index] = item;
                }}
                type="button"
                role="menuitem"
                data-testid="mc-board-previous-plan"
                data-plan-id={previous.id}
                className="ct-menu-row mc-board-plan-menuitem"
                onClick={() => choose(previous.id)}
              >
                <span className="mc-board-plan-menuitem-title">{previous.title}</span>
                <span data-testid="mc-board-previous-plan-date" className="mc-board-plan-list-date">
                  {formatPlanDate(previous.updated_at)}
                </span>
              </button>
            ))}
          </div>
      </AnchoredPopover>
    </div>
  );
}

const PREVIOUS_PLANS_PREVIEW = 5;

/** The plan card while the team has no active plan: the plans it ran before, newest first. */
export function NoActivePlanCard({
  previousPlans,
  onOpenPlan,
}: {
  previousPlans: readonly Plan[];
  onOpenPlan?: (planId: string) => void;
}) {
  const [showAll, setShowAll] = useState(false);
  const listId = useId();
  const visible = showAll ? previousPlans : previousPlans.slice(0, PREVIOUS_PLANS_PREVIEW);
  return (
    <BoardPanel testId="mc-board-plan" title="Plan">
      <BoardPanelSection>
        <p data-testid="mc-board-plan-empty" className="mc-board-plan-heading">
          No active plan
        </p>
      </BoardPanelSection>
      {previousPlans.length > 0 && (
        <BoardPanelSection testId="mc-board-previous-plans" label="Previous plans">
          <PreviousPlanList id={listId} plans={visible} onOpenPlan={onOpenPlan} />
          <ShowAll
            total={previousPlans.length}
            limit={PREVIOUS_PLANS_PREVIEW}
            open={showAll}
            controls={listId}
            testId="mc-board-previous-plans-more"
            onToggle={() => setShowAll((value) => !value)}
          />
        </BoardPanelSection>
      )}
    </BoardPanel>
  );
}

/** The active plan: what it is for, where its tasks stand, and the plans the team ran before it. */
export function PlanCard({
  plan,
  counts,
  bounces,
  previousPlans = [],
  onOpenPlan,
  onOpenPlanView,
  api,
}: {
  plan: Plan;
  api?: ApiConfig;
  counts: PlanTaskCounts;
  bounces: BounceSummary;
  previousPlans?: readonly Plan[];
  onOpenPlan?: (planId: string) => void;
  onOpenPlanView?: () => void;
}) {
  const progress = progressReading(counts);
  const heading = plan.title;
  const goal = planGoal(plan);
  const draft = planDraftState(plan);
  return (
    <BoardPanel
      testId="mc-board-plan"
      title="Plan"
      meta={<PlanModeControls plan={plan} api={api} reason="Paused from Mission Control" />}
    >
      <BoardPanelSection>
        {onOpenPlanView ? (
          <button
            type="button"
            data-testid="mc-board-plan-heading"
            className="mc-board-plan-heading mc-board-plan-link"
            title={heading}
            onClick={onOpenPlanView}
          >
            {heading}
          </button>
        ) : (
          <p data-testid="mc-board-plan-heading" className="mc-board-plan-heading" title={heading}>
            {heading}
          </p>
        )}
        {draft && (
          <p className="mc-board-chips">
            <span data-testid="mc-board-plan-draft" data-planning-state={draft} className="mc-board-chip">
              {`Draft · ${PLANNING_STATE_LABEL[draft]}`}
            </span>
          </p>
        )}
        {goal && <PlanGoal goal={goal} onOpen={onOpenPlanView} />}
      </BoardPanelSection>
      <BoardPanelSection>
        <PlanProgressStats progress={progress} bounces={bounces} />
      </BoardPanelSection>
      <BoardPanelSection>
        <PlanProgressBar progress={progress} />
      </BoardPanelSection>
      {previousPlans.length > 0 && <PreviousPlansMenu plans={previousPlans} onOpenPlan={onOpenPlan} />}
    </BoardPanel>
  );
}
