import { Fragment } from "react";
import type { Plan } from "../types";
import { splitRefs } from "./missionControlBoardModel";

export interface BoardRef {
  label: string;
  open?: () => void;
}

/** Plan and task ids the Board can name, each with its title and, when a handler exists, a way to open it. */
export type BoardRefs = ReadonlyMap<string, BoardRef>;

export function boardRefs(
  plans: readonly Plan[],
  onOpenTask?: (taskId: string) => void,
  onOpenPlan?: (planId: string) => void,
): BoardRefs {
  const refs = new Map<string, BoardRef>();
  for (const plan of plans) {
    refs.set(plan.id, { label: plan.title, open: onOpenPlan && (() => onOpenPlan(plan.id)) });
    for (const task of plan.tasks) {
      refs.set(task.id, { label: task.name, open: onOpenTask && (() => onOpenTask(task.id)) });
    }
  }
  return refs;
}

/** Stored text with known plan and task ids shown by title; unknown ids stay as written. */
export function RefText({ text, refs }: { text: string; refs: BoardRefs }) {
  return (
    <>
      {splitRefs(text).map((part, index) => {
        if (part.kind === "text") return <Fragment key={index}>{part.text}</Fragment>;
        const ref = refs.get(part.id);
        if (!ref) return <Fragment key={index}>{part.id}</Fragment>;
        if (!ref.open) {
          return (
            <span key={index} data-testid="mc-board-ref" className="mc-board-ref-label" title={part.id}>
              {ref.label}
            </span>
          );
        }
        const open = ref.open;
        return (
          <button
            key={index}
            type="button"
            data-testid="mc-board-ref"
            data-ref-id={part.id}
            className="mc-board-ref"
            title={part.id}
            onClick={(event) => {
              event.stopPropagation();
              open();
            }}
          >
            {ref.label}
          </button>
        );
      })}
    </>
  );
}
