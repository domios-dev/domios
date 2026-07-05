import { Repeat, Swords } from "lucide-react";
import type { CompetitionForTaskResult } from "./competitionForTask";
import type { LoopForTaskResult } from "./loopForTask";

export interface TaskExecutionBadgesProps {
  loop?: LoopForTaskResult | null;
  competition?: CompetitionForTaskResult | null;
  className?: string;
}

export function TaskExecutionBadges({ loop, competition, className }: TaskExecutionBadgesProps) {
  if (!loop && !competition) return null;

  return (
    <span className={["task-execution-badges", className].filter(Boolean).join(" ")}>
      {loop && (
        <span
          data-testid="task-loop-badge"
          className={`task-loop-badge${loop.warning ? " task-loop-badge-warning" : ""}`}
          title={loop.tooltip}
          aria-label={`Loop iteration ${loop.count}`}
        >
          <Repeat size={10} strokeWidth={2.5} aria-hidden className="task-loop-badge-glyph" />
          <span className="task-loop-badge-count">×{loop.count}</span>
        </span>
      )}
      {competition && (
        <span
          data-testid="task-compete-badge"
          className="task-compete-badge"
          title={competition.tooltip}
          aria-label={`Competitive execution ×${competition.count}`}
        >
          <Swords size={10} strokeWidth={2.5} aria-hidden className="task-compete-badge-glyph" />
          <span className="task-compete-badge-count">×{competition.count}</span>
        </span>
      )}
    </span>
  );
}
