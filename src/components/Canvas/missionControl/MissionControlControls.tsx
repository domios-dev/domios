import { useRef, useState } from "react";
import { Check, Pause, Play, Trash2 } from "lucide-react";
import { DropdownMenu, type DropdownMenuItem } from "../../shared/DropdownMenu";
import type {
  PlanLifecycleConfirm,
  PlanLifecycleKind,
  PlanLifecyclePreviewLoader,
} from "../planLifecycle";
import { MissionControlActionDialog, type MissionControlActionRequest } from "./MissionControlActionDialog";
import { MissionControlPlanLifecycleDialog } from "./MissionControlPlanLifecycleDialog";

export type MissionControlPlanAction = (planId: string) => void | Promise<void>;
export type MissionControlTaskAction = (planId: string, taskId: string) => void | Promise<void>;

export interface MissionControlControlsProps {
  scope: "board" | "task";
  planId: string;
  taskId?: string;
  taskName?: string;
  planPaused?: boolean;
  onApprovePlan?: MissionControlPlanAction;
  onPreviewPlanLifecycle?: PlanLifecyclePreviewLoader;
  onConfirmPlanLifecycle?: PlanLifecycleConfirm;
  onClearEverything?: MissionControlPlanAction;
  onDeleteTask?: MissionControlTaskAction;
}

const TASK_TRIGGER_CLASS =
  "w-5 h-5 text-[var(--ct-muted-fg)] flex items-center justify-center rounded hover:text-[var(--ct-fg)] hover:bg-[var(--ct-border)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)] transition-colors";

function stopInteraction(event: { stopPropagation: () => void }) {
  event.stopPropagation();
}

export function MissionControlControls({
  scope,
  planId,
  taskId,
  taskName,
  planPaused = false,
  onApprovePlan,
  onPreviewPlanLifecycle,
  onConfirmPlanLifecycle,
  onClearEverything,
  onDeleteTask,
}: MissionControlControlsProps) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const [lifecycle, setLifecycle] = useState<PlanLifecycleKind | null>(null);
  const [request, setRequest] = useState<MissionControlActionRequest | null>(null);
  const itemTestId = (key: string) => `mission-control-${scope}-menu-item-${key}`;
  const taskLabel = taskName ?? taskId;

  const boardItems = (): DropdownMenuItem[] => [
    {
      label: "Approve plan",
      icon: <Check size={11} strokeWidth={2} />,
      "data-testid": itemTestId("approve"),
      onClick: () =>
        setRequest({
          key: "approve-plan",
          title: "Approve plan",
          message: `Approve plan "${planId}"?`,
          confirmLabel: "Approve",
          run: () => onApprovePlan?.(planId),
        }),
    },
    planPaused
      ? {
          label: "Resume plan",
          icon: <Play size={11} strokeWidth={2} />,
          "data-testid": itemTestId("resume"),
          onClick: () => setLifecycle("resume"),
        }
      : {
          label: "Pause plan",
          icon: <Pause size={11} strokeWidth={2} />,
          "data-testid": itemTestId("pause"),
          onClick: () => setLifecycle("pause"),
        },
    {
      label: "Clear everything",
      icon: <Trash2 size={11} strokeWidth={2} />,
      danger: true,
      "data-testid": itemTestId("clear"),
      onClick: () =>
        setRequest({
          key: "clear-everything",
          title: "Clear everything",
          message: `Clear everything in plan "${planId}"? This cannot be undone.`,
          confirmLabel: "Clear everything",
          destructive: true,
          run: () => onClearEverything?.(planId),
        }),
    },
  ];

  const taskItems = (id: string): DropdownMenuItem[] => {
    const items: DropdownMenuItem[] = [];
    if (onDeleteTask) {
      items.push({
        label: "Delete",
        icon: <Trash2 size={11} strokeWidth={2} />,
        danger: true,
        "data-testid": itemTestId("delete"),
        onClick: () =>
          setRequest({
            key: "delete-task",
            title: "Delete task",
            message: `Delete "${taskLabel}" from this plan? This cannot be undone.`,
            confirmLabel: "Delete",
            destructive: true,
            run: () => onDeleteTask?.(planId, id),
          }),
      });
    }
    return items;
  };

  const items = scope === "board" ? boardItems() : taskId ? taskItems(taskId) : [];
  if (items.length === 0 && !lifecycle && !request) return null;

  const restoreTriggerFocus = () => triggerRef.current?.focus();
  const triggerLabel = scope === "board" ? "Board actions" : `Actions for task ${taskLabel}`;

  return (
    <div
      className="mission-control-actions"
      data-testid={`mission-control-${scope}-actions`}
      data-plan-id={planId}
      data-task-id={taskId}
      onClick={stopInteraction}
      onKeyDown={stopInteraction}
      onPointerDown={stopInteraction}
    >
      {items.length > 0 && (
        <DropdownMenu
          items={items}
          menuLabel={scope === "board" ? "Board actions" : `Task actions for ${taskLabel}`}
          menuTestId={`mission-control-${scope}-menu`}
          className="flex items-center"
          trigger={
            <button
              ref={triggerRef}
              type="button"
              className={scope === "board" ? "mission-control-actions-trigger" : TASK_TRIGGER_CLASS}
              data-testid={`mission-control-${scope}-menu-trigger`}
              aria-label={triggerLabel}
              aria-haspopup="menu"
              title={triggerLabel}
            >
              {scope === "board" ? (
                "⋯"
              ) : (
                <span aria-hidden className="text-[13px] leading-none font-bold">
                  ⋮
                </span>
              )}
            </button>
          }
        />
      )}
      {lifecycle && (
        <MissionControlPlanLifecycleDialog
          kind={lifecycle}
          planId={planId}
          loadPreview={onPreviewPlanLifecycle}
          onConfirm={onConfirmPlanLifecycle}
          onClose={() => {
            setLifecycle(null);
            restoreTriggerFocus();
          }}
        />
      )}
      {request && (
        <MissionControlActionDialog
          request={request}
          onClose={() => {
            setRequest(null);
            restoreTriggerFocus();
          }}
        />
      )}
    </div>
  );
}
