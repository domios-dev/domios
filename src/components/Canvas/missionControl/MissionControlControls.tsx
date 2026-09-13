import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import type {
  PlanLifecycleConfirm,
  PlanLifecycleKind,
  PlanLifecyclePreviewLoader,
} from "../planLifecycle";
import { MissionControlPlanLifecycleDialog } from "./MissionControlPlanLifecycleDialog";

export type MissionControlPlanAction = (planId: string) => void | Promise<void>;
export type MissionControlTaskAction = (planId: string, taskId: string) => void | Promise<void>;

export interface MissionControlControlsProps {
  scope: "board" | "task";
  planId: string;
  taskId?: string;
  planPaused?: boolean;
  onApprovePlan?: MissionControlPlanAction;
  onPreviewPlanLifecycle?: PlanLifecyclePreviewLoader;
  onConfirmPlanLifecycle?: PlanLifecycleConfirm;
  onClearEverything?: MissionControlPlanAction;
  onDeleteTask?: MissionControlTaskAction;
}

type MenuAction =
  | {
      label: string;
      confirmation: string;
      destructive?: boolean;
      run: () => void | Promise<void>;
    }
  | {
      label: string;
      lifecycle: PlanLifecycleKind;
      destructive?: undefined;
    };

function stopInteraction(event: { stopPropagation: () => void }) {
  event.stopPropagation();
}

export function MissionControlControls({
  scope,
  planId,
  taskId,
  planPaused = false,
  onApprovePlan,
  onPreviewPlanLifecycle,
  onConfirmPlanLifecycle,
  onClearEverything,
  onDeleteTask,
}: MissionControlControlsProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const [open, setOpen] = useState(false);
  const [lifecycle, setLifecycle] = useState<PlanLifecycleKind | null>(null);

  useEffect(() => {
    if (!open) return;
    rootRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
    const onDown = (event: globalThis.MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const actions: MenuAction[] = scope === "board"
    ? [
        {
          label: "Approve plan",
          confirmation: `Approve plan "${planId}"?`,
          run: () => onApprovePlan?.(planId),
        },
        planPaused
          ? { label: "Resume plan", lifecycle: "resume" }
          : { label: "Pause plan", lifecycle: "pause" },
        {
          label: "Clear everything",
          confirmation: `Clear everything in plan "${planId}"?`,
          destructive: true,
          run: () => onClearEverything?.(planId),
        },
      ]
    : taskId
      ? [
          {
            label: "Delete",
            confirmation: `Delete task "${taskId}" from plan "${planId}"?`,
            destructive: true,
            run: () => onDeleteTask?.(planId, taskId),
          },
        ]
      : [];

  const close = (restoreFocus = false) => {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      stopInteraction(event);
      close(true);
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    stopInteraction(event);
    const items = Array.from(
      rootRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [],
    );
    if (items.length === 0) return;
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const delta = event.key === "ArrowDown" ? 1 : -1;
    items[(current + delta + items.length) % items.length]?.focus();
  };

  const onAction = (action: MenuAction, event: MouseEvent<HTMLButtonElement>) => {
    stopInteraction(event);
    if ("lifecycle" in action) {
      close(false);
      setLifecycle(action.lifecycle);
      return;
    }
    close(true);
    if (!window.confirm(action.confirmation)) return;
    void action.run();
  };

  const closeLifecycle = () => {
    setLifecycle(null);
    triggerRef.current?.focus();
  };

  return (
    <div
      ref={rootRef}
      className="mission-control-actions"
      data-testid={`mission-control-${scope}-actions`}
      data-plan-id={planId}
      data-task-id={taskId}
      onClick={stopInteraction}
      onKeyDown={stopInteraction}
      onPointerDown={stopInteraction}
    >
      <button
        ref={triggerRef}
        type="button"
        className="mission-control-actions-trigger"
        data-testid={`mission-control-${scope}-menu-trigger`}
        aria-label={scope === "board" ? "Board actions" : `Actions for task ${taskId}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        ⋯
      </button>
      {open && (
        <div
          className="mission-control-actions-menu"
          data-testid={`mission-control-${scope}-menu`}
          role="menu"
          aria-label={scope === "board" ? "Board actions" : `Task actions for ${taskId}`}
          onKeyDown={onMenuKeyDown}
        >
          {actions.map((action) => (
            <button
              key={action.label}
              type="button"
              role="menuitem"
              aria-haspopup={"lifecycle" in action ? "dialog" : undefined}
              className={`mission-control-actions-menuitem${action.destructive ? " mission-control-actions-menuitem-danger" : ""}`}
              onClick={(event) => onAction(action, event)}
            >
              {action.label}
            </button>
          ))}
        </div>
      )}
      {lifecycle && (
        <MissionControlPlanLifecycleDialog
          kind={lifecycle}
          planId={planId}
          loadPreview={onPreviewPlanLifecycle}
          onConfirm={onConfirmPlanLifecycle}
          onClose={closeLifecycle}
        />
      )}
    </div>
  );
}
