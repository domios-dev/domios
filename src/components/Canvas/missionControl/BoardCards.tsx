import { useEffect, useId, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { Plan } from "../types";
import {
  formatCount,
  formatPlanDate,
  planGoal,
  progressReading,
} from "./missionControlBoardModel";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection, ShowAll } from "./BoardPanel";
import type { BoardRefs } from "./BoardRefText";
import {
  NeedsYouNotificationRow,
  useNeedsYouNotificationItems,
  type NeedsYouDeepLinks,
} from "../../Notifications/NeedsYouNotificationRow";
import type { BounceSummary, PlanTaskCounts } from "./missionControlState";

export { ModelsCard } from "./BoardModelsCard";
export { SpendCard } from "./BoardSpendCard";

const WARN_STATUSES = new Set(["blocked", "paused"]);

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

function Pair({
  label,
  testId,
  warn = false,
  unit,
  children,
}: {
  label: string;
  testId: string;
  warn?: boolean;
  unit?: string;
  children: ReactNode;
}) {
  return (
    <div className="mc-board-pair">
      <dt>{label}</dt>
      <dd>
        <b data-testid={testId} className={warn ? "mc-board-warn" : undefined}>
          {children}
        </b>
        {unit && ` ${unit}`}
      </dd>
    </div>
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

const MENU_EDGE_PX = 8;
const MENU_GAP_PX = 4;
const MENU_MAX_HEIGHT_PX = 256;
const MENU_MIN_WIDTH_PX = 224;

/** Anchors the menu to its trigger in the window: above it when there is room, else below, never past an edge. */
function menuLayerStyle(anchor: DOMRect): CSSProperties {
  const left = Math.max(MENU_EDGE_PX, Math.min(anchor.left, window.innerWidth - MENU_EDGE_PX - MENU_MIN_WIDTH_PX));
  const maxWidth = Math.max(0, window.innerWidth - left - MENU_EDGE_PX);
  const roomAbove = anchor.top - MENU_GAP_PX - MENU_EDGE_PX;
  const roomBelow = window.innerHeight - anchor.bottom - MENU_GAP_PX - MENU_EDGE_PX;
  return roomAbove >= MENU_MAX_HEIGHT_PX || roomAbove >= roomBelow
    ? { left, maxWidth, bottom: window.innerHeight - anchor.top + MENU_GAP_PX, maxHeight: Math.min(MENU_MAX_HEIGHT_PX, roomAbove) }
    : { left, maxWidth, top: anchor.bottom + MENU_GAP_PX, maxHeight: Math.min(MENU_MAX_HEIGHT_PX, roomBelow) };
}

/** A quiet footer trigger whose menu of previous plans floats above the Board; choosing one opens its detail. */
function PreviousPlansMenu({
  plans,
  onOpenPlan,
}: {
  plans: readonly Plan[];
  onOpenPlan?: (planId: string) => void;
}) {
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const open = anchor != null;
  const menuId = useId();
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const itemsRef = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    if (!open) return;
    itemsRef.current[0]?.focus();
    const owns = (target: EventTarget | null) =>
      target instanceof Node && Boolean(menuRef.current?.contains(target) || triggerRef.current?.contains(target));
    const dismiss = (event: Event) => {
      if (!owns(event.target)) setAnchor(null);
    };
    document.addEventListener("mousedown", dismiss);
    window.addEventListener("scroll", dismiss, true);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("mousedown", dismiss);
      window.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [open]);

  const close = () => {
    setAnchor(null);
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
      setAnchor(null);
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
        onClick={() => setAnchor((current) => (current ? null : triggerRef.current?.getBoundingClientRect() ?? null))}
      >
        Previous plans <span className="mc-board-plan-count">{formatCount(plans.length)}</span>
      </button>
      {anchor &&
        createPortal(
          <div
            ref={menuRef}
            id={menuId}
            role="menu"
            aria-label="Previous plans"
            data-testid="mc-board-previous-plans-menu"
            className="mc-board-layer mc-board-plan-menu"
            style={menuLayerStyle(anchor)}
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
                className="mc-board-plan-menuitem"
                onClick={() => choose(previous.id)}
              >
                <span className="mc-board-plan-menuitem-title">{previous.title}</span>
                <span data-testid="mc-board-previous-plan-date" className="mc-board-plan-list-date">
                  {formatPlanDate(previous.updated_at)}
                </span>
              </button>
            ))}
          </div>,
          document.body,
        )}
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
  onOpenTasksView,
}: {
  plan: Plan;
  counts: PlanTaskCounts;
  bounces: BounceSummary;
  previousPlans?: readonly Plan[];
  onOpenPlan?: (planId: string) => void;
  onOpenPlanView?: () => void;
  onOpenTasksView?: () => void;
}) {
  const progress = progressReading(counts);
  const bounced = bounces.tasks_on_second_attempt;
  const heading = plan.title;
  const goal = planGoal(plan);
  const taskCount = (
    <>
      <span data-testid="mc-board-tasks-amount" className="mc-board-num">
        <AnimatedNumber value={progress.total} format={formatCount} />
      </span>
      {progress.total === 1 ? " task" : " tasks"}
    </>
  );
  return (
    <BoardPanel
      testId="mc-board-plan"
      title="Plan"
      meta={
        onOpenTasksView ? (
          <button
            type="button"
            data-testid="mc-board-plan-tasks-link"
            className="mc-board-meta-link"
            onClick={onOpenTasksView}
          >
            {taskCount}
          </button>
        ) : (
          taskCount
        )
      }
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
        {goal && (
          <p data-testid="mc-board-plan-goal" className="mc-board-plan-goal" title={goal}>
            {goal}
          </p>
        )}
        <div data-testid="mc-board-task-statuses">
          {progress.active.length === 0 ? (
            <p className="mc-board-note">No tasks yet.</p>
          ) : (
            <ul className="mc-board-statuses" role="list">
              {progress.active.map((status) => (
                <li key={status.key} data-testid={`mc-board-task-status-${status.key}`} className="mc-board-status">
                  <b
                    data-testid={`mc-board-task-status-${status.key}-count`}
                    className={`mc-board-figure${WARN_STATUSES.has(status.key) ? " mc-board-warn" : ""}`}
                  >
                    <AnimatedNumber value={status.count} format={formatCount} />
                  </b>
                  <span>{status.label}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
        <dl data-testid="mc-board-quality" className="mc-board-pairs">
          <Pair label="bounced" testId="mc-board-bounces-amount" warn={bounced > 0}>
            <AnimatedNumber value={bounced} format={formatCount} />
          </Pair>
          <Pair
            label="first try"
            testId="mc-board-done-first-try-amount"
            unit={`of ${formatCount(bounces.total_attempts)}`}
          >
            <AnimatedNumber value={bounces.first_try_done} format={formatCount} />
          </Pair>
        </dl>
      </BoardPanelSection>
      {previousPlans.length > 0 && <PreviousPlansMenu plans={previousPlans} onOpenPlan={onOpenPlan} />}
    </BoardPanel>
  );
}
