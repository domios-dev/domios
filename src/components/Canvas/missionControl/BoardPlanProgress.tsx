import { useId, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { AnimatedNumber } from "./AnimatedNumber";
import {
  doneCaption,
  formatCount,
  openCaption,
  progressRatio,
  type ProgressRatio,
  type ProgressReading,
} from "./missionControlBoardModel";
import type { BounceSummary } from "./missionControlState";

function useLineClamped(ref: RefObject<HTMLElement | null>, text: string): boolean {
  const [clamped, setClamped] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const measure = () => setClamped(el.scrollHeight > el.clientHeight + 1);
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    measure();
    return () => observer.disconnect();
  }, [ref, text]);
  return clamped;
}

/** The goal at reading size, cut to three lines until the owner asks for the rest. */
export function PlanGoal({ goal }: { goal: string }) {
  const ref = useRef<HTMLParagraphElement | null>(null);
  const [expanded, setExpanded] = useState(false);
  const clamped = useLineClamped(ref, goal);
  const goalId = useId();
  return (
    <>
      <p
        ref={ref}
        id={goalId}
        data-testid="mc-board-plan-goal"
        data-expanded={expanded ? "true" : undefined}
        className="mc-board-plan-goal"
        title={goal}
      >
        {goal}
      </p>
      {(clamped || expanded) && (
        <button
          type="button"
          data-testid="mc-board-plan-goal-toggle"
          className="mc-board-more"
          aria-expanded={expanded}
          aria-controls={goalId}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? "Show less" : "Read the whole goal"}
        </button>
      )}
    </>
  );
}

type FigureTone = "danger" | "calm" | undefined;

function RatioFigure({ ratio, testId }: { ratio: ProgressRatio; testId?: string }) {
  if (ratio.whole === 0) return <span data-testid={testId}>–</span>;
  if (ratio.percent !== null) {
    return <AnimatedNumber testId={testId} value={ratio.percent} format={(value) => `${formatCount(value)}%`} />;
  }
  return (
    <span data-testid={testId}>
      <AnimatedNumber value={ratio.part} format={formatCount} />/{formatCount(ratio.whole)}
    </span>
  );
}

function Stat({ label, tone, figure, caption }: { label: string; tone: FigureTone; figure: ReactNode; caption: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>
        <span className="mc-board-progress-figure" data-tone={tone}>
          {figure}
        </span>
        <span className="mc-board-progress-of">{caption}</span>
      </dd>
    </div>
  );
}

/** Three readings a plan is asked about: how much is done, what is stuck, and how often work lands first time. */
export function PlanProgressStats({ progress, bounces }: { progress: ProgressReading; bounces: BounceSummary }) {
  const firstTry = progressRatio(bounces.first_try_done, bounces.total_attempts);
  const bounced = bounces.tasks_on_second_attempt;
  return (
    <dl data-testid="mc-board-progress-stats" className="mc-board-progress-stats">
      <Stat
        label="done"
        tone={progress.done.part === 0 ? "calm" : undefined}
        figure={<RatioFigure ratio={progress.done} testId="mc-board-done-amount" />}
        caption={doneCaption(progress.done)}
      />
      <Stat
        label="blocked"
        tone={progress.blocked > 0 ? "danger" : "calm"}
        figure={<AnimatedNumber testId="mc-board-blocked-amount" value={progress.blocked} format={formatCount} />}
        caption={openCaption(progress.open)}
      />
      <Stat
        label="first try"
        tone={firstTry.whole === 0 ? "calm" : undefined}
        figure={<RatioFigure ratio={firstTry} testId="mc-board-done-first-try-amount" />}
        caption={
          firstTry.whole === 0 ? (
            "no attempts yet"
          ) : bounced === 0 ? (
            "none bounced"
          ) : (
            <span title={`${formatCount(bounces.first_try_done)} of ${formatCount(bounces.total_attempts)} attempts done on the first try`}>
              <span data-testid="mc-board-bounces-amount">{formatCount(bounced)}</span> bounced
            </span>
          )
        }
      />
    </dl>
  );
}

/** The whole state mix in one bar, with a legend that names each part and its count. */
export function PlanProgressBar({ progress }: { progress: ProgressReading }) {
  const summary = progress.segments.map((status) => `${formatCount(status.count)} ${status.label}`).join(", ");
  return (
    <div data-testid="mc-board-task-statuses" className="mc-board-progress">
      <div
        role="img"
        aria-label={progress.total === 0 ? "No tasks yet" : `${formatCount(progress.total)} tasks: ${summary}`}
        className="mc-board-progress-bar"
      >
        {progress.segments.length === 0 ? (
          <span className="mc-board-progress-empty" />
        ) : (
          progress.segments.map((status) => (
            <span
              key={status.key}
              data-status={status.key}
              className="mc-board-progress-seg"
              style={{ flexGrow: status.count }}
              title={`${formatCount(status.count)} ${status.label}`}
            />
          ))
        )}
      </div>
      {progress.segments.length === 0 ? (
        <p className="mc-board-note">No tasks yet.</p>
      ) : (
        <ul className="mc-board-chart-legend mc-board-progress-legend" role="list">
          {progress.segments.map((status) => (
            <li
              key={status.key}
              data-testid={`mc-board-task-status-${status.key}`}
              data-status={status.key}
              className="mc-board-chart-legend-item"
            >
              <span className="mc-board-swatch" data-status={status.key} aria-hidden="true" />
              <b data-testid={`mc-board-task-status-${status.key}-count`}>
                <AnimatedNumber value={status.count} format={formatCount} />
              </b>
              <span>{status.label}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
