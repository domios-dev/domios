import { Fragment } from "react";
import { taskReferenceLabel } from "../taskDisplayKey";
import type { Plan } from "../types";
import { splitRefs } from "./missionControlBoardModel";

export interface BoardRef {
  label: string;
  open?: () => void;
}

/** Plan and task ids the Board can name, each with its label and, when a handler exists, a way to open it. */
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
      refs.set(task.id, { label: taskReferenceLabel(task), open: onOpenTask && (() => onOpenTask(task.id)) });
    }
  }
  return refs;
}

/** Stored text read aloud or copied: known ids become their labels, unknown ids stay as written. */
export function refPlainText(text: string, refs: BoardRefs): string {
  return splitRefs(text)
    .map((part) => (part.kind === "text" ? part.text : refs.get(part.id)?.label ?? part.id))
    .join("");
}

/** One known plan or task by its label, as a link when it can be opened; an unknown id shows the fallback. */
export function RefLabel({
  id,
  refs,
  fallback,
  format = (label) => label,
}: {
  id: string;
  refs: BoardRefs;
  fallback?: string;
  format?: (label: string) => string;
}) {
  const ref = refs.get(id);
  if (!ref) return <>{fallback ?? id}</>;
  const shown = format(ref.label);
  if (!ref.open) {
    return (
      <span data-testid="mc-board-ref" className="mc-board-ref-label" title={id}>
        {shown}
      </span>
    );
  }
  const open = ref.open;
  return (
    <button
      type="button"
      data-testid="mc-board-ref"
      data-ref-id={id}
      className="mc-board-ref"
      title={shown === ref.label ? id : `${ref.label}\n${id}`}
      onClick={(event) => {
        event.stopPropagation();
        open();
      }}
    >
      {shown}
    </button>
  );
}

/** Stored text with known plan and task ids shown by label; unknown ids stay as written. */
export function RefText({ text, refs }: { text: string; refs: BoardRefs }) {
  return (
    <>
      {splitRefs(text).map((part, index) =>
        part.kind === "text"
          ? <Fragment key={index}>{part.text}</Fragment>
          : <RefLabel key={index} id={part.id} refs={refs} />,
      )}
    </>
  );
}
