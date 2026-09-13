import { useId, useState, type ReactNode } from "react";
import type { Plan } from "../types";
import { formatCount, needsYouItems, planHeading, progressReading } from "./missionControlBoardModel";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection, Facts, ShowAll } from "./BoardPanel";
import { RefText, type BoardRefs } from "./BoardRefText";
import {
  NeedsYouNotificationRow,
  useNeedsYouNotificationItems,
  type NeedsYouDeepLinks,
} from "../../Notifications/NeedsYouNotificationRow";
import type { BounceSummary, HeldForDecision, PlanTaskCounts } from "./missionControlState";

export { ModelsCard } from "./BoardModelsCard";
export { SpendCard } from "./BoardSpendCard";

const WARN_STATUSES = new Set(["blocked", "paused"]);

const NEEDS_YOU_PREVIEW = 3;

export function NeedsYouPanel({
  held,
  refs = new Map(),
  deepLinks,
}: {
  held: HeldForDecision;
  refs?: BoardRefs;
  deepLinks?: NeedsYouDeepLinks;
}) {
  const items = needsYouItems(held);
  const notificationItems = useNeedsYouNotificationItems(deepLinks);
  const [showAll, setShowAll] = useState(false);
  const listId = useId();
  const total = notificationItems.length + items.length;
  if (total === 0) return null;
  const limit = showAll ? total : NEEDS_YOU_PREVIEW;
  const visibleNotifications = notificationItems.slice(0, limit);
  const visible = items.slice(0, Math.max(0, limit - visibleNotifications.length));
  return (
    <BoardPanel testId="mc-board-needs-you" title="Needs you">
      <div className="mc-board-needs">
        <span data-testid="mc-board-needs-you-count" className="mc-board-figure mc-board-warn">
          <AnimatedNumber value={total} format={formatCount} />
        </span>
        <div className="mc-board-needs-body">
          <ul id={listId} className="mc-board-needs-list">
            {deepLinks && visibleNotifications.map((item) => (
              <NeedsYouNotificationRow
                key={item.key}
                item={item}
                tabs={deepLinks.tabs}
                onSelectTab={deepLinks.onSelectTab}
              />
            ))}
            {visible.map((item) => (
              <li key={item.key} data-testid="mc-board-needs-you-item" data-kind={item.kind}>
                <p className="mc-board-needs-title">
                  <RefText text={item.title} refs={refs} />
                </p>
                <Facts items={item.facts.map((fact, index) => ({ key: String(index), node: fact }))} />
              </li>
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
              {planHeading(previous)}
            </button>
          ) : (
            <span data-testid="mc-board-previous-plan">{planHeading(previous)}</span>
          )}
        </li>
      ))}
    </ul>
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
  const [showPrevious, setShowPrevious] = useState(false);
  const previousId = useId();
  const bounced = bounces.tasks_on_second_attempt;
  const heading = planHeading(plan);
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
      {previousPlans.length > 0 && (
        <BoardPanelSection testId="mc-board-previous-plans">
          <button
            type="button"
            data-testid="mc-board-previous-plans-toggle"
            className="mc-board-more"
            aria-expanded={showPrevious}
            aria-controls={showPrevious ? previousId : undefined}
            onClick={() => setShowPrevious((value) => !value)}
          >
            {showPrevious ? "Hide previous plans" : "Show previous plans"}
          </button>
          {showPrevious && <PreviousPlanList id={previousId} plans={previousPlans} onOpenPlan={onOpenPlan} />}
        </BoardPanelSection>
      )}
    </BoardPanel>
  );
}
