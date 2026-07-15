import { useCallback, useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { ChevronLeft, X } from "lucide-react";
import { DetailContent } from "../canvasRender/DetailContent";
import type { ApiConfig, Pane } from "../../../lib/types";
import type { Plan, Section, Task } from "../types";
import type { SectionNavArgs } from "../nodeRegistry";
import { competitionForTask } from "./competitionForTask";
import { loopForTask } from "./loopForTask";
import { TaskExecutionBadges } from "./TaskExecutionBadges";
import { PlanModeControls } from "./PlanModeControls";

export type PlanDetailKind = "task" | "plan" | "spec";

export interface PlanDetailOverlayProps {
  kind: PlanDetailKind;
  task?: Task;
  plan?: Plan;
  spec?: Plan;
  specSections?: Section[];
  planId?: string;
  api?: ApiConfig;
  panes?: Record<string, Pane>;
  onOpenPane?: (paneId: string) => void;
  onOpenTask?: (taskId: string) => void;
  onClose: () => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  renderTaskAttachmentThumbnails?: (task: Task) => ReactNode;
  /** Preserves canvas Playwright hooks when set to `canvas-detail`. */
  testIdPrefix?: string;
  /** Show back button and handle keyboard back (Alt/Cmd+Left). */
  canGoBack?: boolean;
  onBack?: () => void;
  /** Scroll the body to this offset after mount/navigation (best-effort). */
  restoreScroll?: number;
  /** Ref assigned to the body scroll container so callers can read scrollTop. */
  scrollCaptureRef?: { current: HTMLDivElement | null };
}

function detailTitle(kind: PlanDetailKind, task?: Task, plan?: Plan, spec?: Plan): string {
  if (kind === "task") return task?.name ?? "Task";
  if (kind === "spec") return `${spec?.title ?? "Spec"} · spec`;
  if (kind === "plan") return plan?.title ?? "Plan";
  return "Detail";
}

const FOCUSABLE = 'a[href],button:not([disabled]),input,textarea,select,[tabindex]:not([tabindex="-1"])';
type DetailKeyEvent = {
  key: string;
  shiftKey: boolean;
  target: EventTarget | null;
  preventDefault: () => void;
  stopPropagation: () => void;
};

export function PlanDetailOverlay({
  kind,
  task,
  plan,
  spec,
  specSections,
  planId,
  api,
  panes,
  onOpenPane,
  onOpenTask,
  onClose,
  onSectionClick,
  onSectionFrame,
  renderTaskAttachmentThumbnails,
  testIdPrefix = "plan-detail",
  canGoBack = false,
  onBack,
  restoreScroll,
  scrollCaptureRef,
}: PlanDetailOverlayProps) {
  const panelRef = useRef<HTMLDivElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  const assignBodyRef = useCallback(
    (el: HTMLDivElement | null) => {
      bodyRef.current = el;
      if (scrollCaptureRef) scrollCaptureRef.current = el;
    },
    [scrollCaptureRef],
  );

  useEffect(() => {
    if (typeof restoreScroll === "number" && bodyRef.current) {
      bodyRef.current.scrollTop = restoreScroll;
    }
  }, [restoreScroll]);
  const tid = (suffix: string) => `${testIdPrefix}-${suffix}`;
  const isTask = kind === "task";

  const onKeyDown = useCallback(
    (e: DetailKeyEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      const ke = e as unknown as KeyboardEvent;
      if (e.key === "ArrowLeft" && (ke.altKey || ke.metaKey) && canGoBack && onBack) {
        e.preventDefault?.();
        e.stopPropagation();
        onBack();
        return;
      }
      if (e.key !== "Tab" || !panelRef.current) return;
      const items = Array.from(panelRef.current.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => !el.hasAttribute("disabled") && el.getAttribute("aria-hidden") !== "true",
      );
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      const current = document.activeElement as HTMLElement | null;
      const target = e.target instanceof HTMLElement ? e.target : null;
      const active = current && items.includes(current) ? current : target;
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    },
    [onClose, canGoBack, onBack],
  );

  useEffect(() => {
    returnFocusRef.current = (document.activeElement as HTMLElement | null) ?? null;
    panelRef.current?.focus();
    return () => {
      returnFocusRef.current?.focus?.();
    };
  }, []);

  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    const listener = (event: KeyboardEvent) => onKeyDown(event);
    panel.addEventListener("keydown", listener);
    return () => panel.removeEventListener("keydown", listener);
  }, [onKeyDown]);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    };
    document.addEventListener("keydown", listener);
    return () => document.removeEventListener("keydown", listener);
  }, [onClose]);


  const title = detailTitle(kind, task, plan, spec);
  const navPlanId = planId ?? plan?.id ?? spec?.id;
  const loopBadge = isTask && task && plan ? loopForTask(plan, task.id) : null;
  const competeBadge = isTask && task ? competitionForTask(task, panes) : null;
  const onSectionNavigate = (a: SectionNavArgs) => {
    onClose();
    if (onSectionFrame) {
      onSectionFrame(a);
      return;
    }
    onSectionClick?.(a);
  };

  return (
    <div
      data-testid={tid("overlay")}
      role="dialog"
      aria-modal={isTask ? "false" : "true"}
      aria-label={`${title} — detail`}
      onKeyDown={onKeyDown}
      data-detail-mode={isTask ? "slide-over" : "document"}
      onMouseDown={(e) => { if (!isTask && e.target === e.currentTarget) onClose(); }}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 40,
        display: "flex",
        alignItems: "stretch",
        justifyContent: isTask ? "flex-end" : "center",
        padding: isTask ? "0 0 0 12px" : "clamp(12px, 4vh, 56px) clamp(12px, 4vw, 64px)",
        background: isTask ? "transparent" : "color-mix(in srgb, var(--ct-bg) 62%, transparent)",
        backdropFilter: isTask ? "none" : "blur(3px)",
        pointerEvents: isTask ? "none" : "auto",
      }}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        data-testid={tid("panel")}
        style={{
          position: "relative",
          width: isTask ? "clamp(360px, 43vw, 720px)" : "100%",
          maxWidth: isTask ? "calc(100% - 12px)" : 1180,
          height: "100%",
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          borderRadius: isTask ? "var(--ct-radius-lg) 0 0 var(--ct-radius-lg)" : "var(--ct-radius-lg)",
          border: "1px solid var(--ct-border-default)",
          borderRight: isTask ? "none" : "1px solid var(--ct-border-default)",
          background: "var(--ct-bg)",
          boxShadow: "0 24px 80px -20px color-mix(in srgb, var(--ct-shadow, #000) 65%, transparent)",
          overflow: "hidden",
          outline: "none",
          pointerEvents: "auto",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px 10px 16px", borderBottom: "1px solid var(--ct-border-subtle)", flexShrink: 0 }}>
          {canGoBack && (
            <button
              type="button"
              data-testid={tid("back")}
              title="Back (Alt+Left)"
              aria-label="Go back"
              onClick={onBack}
              style={{ display: "flex", alignItems: "center", justifyContent: "center", width: 26, height: 26, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 }}
            >
              <ChevronLeft size={15} />
            </button>
          )}
          <span data-testid={tid("title")} className="plan-task-selectable" style={{ flex: 1, minWidth: 0, fontFamily: "var(--ct-font-ui)", fontWeight: 700, fontSize: "var(--ct-font-size-md, 15px)", color: "var(--ct-fg)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {title}
          </span>
          {isTask && (
            <TaskExecutionBadges loop={loopBadge} competition={competeBadge} className="plan-detail-header-badges" />
          )}
          {kind === "plan" && plan && <PlanModeControls plan={plan} api={api} />}
          <span data-testid={tid("kind")} style={{ fontSize: 11, fontWeight: 700, letterSpacing: "0.06em", textTransform: "uppercase", color: "color-mix(in srgb, var(--ct-fg) 55%, transparent)", flexShrink: 0 }}>
            {kind}
          </span>
          <button
            type="button"
            data-testid={tid("close")}
            title="Close (Esc)"
            aria-label="Close detail"
            onClick={onClose}
            style={{ display: "flex", alignItems: "center", justifyContent: "center", width: 28, height: 28, borderRadius: "var(--ct-radius-sm)", border: "1px solid var(--ct-border-subtle)", background: "var(--ct-bg)", color: "var(--ct-fg)", cursor: "pointer", flexShrink: 0 }}
          >
            <X size={15} />
          </button>
        </div>
        <div ref={assignBodyRef} data-testid={tid("body")} className="plan-detail-selectable" style={{ position: "relative", flex: 1, minHeight: 0, overflowY: "auto" }}>
          <DetailContent
            kind={kind}
            planId={navPlanId}
            api={api}
            task={task}
            plan={plan}
            spec={spec}
            specSections={specSections}
            panes={panes}
            onOpenPane={onOpenPane}
            onOpenTask={onOpenTask}
            onSectionClick={onSectionNavigate}
            onSectionFrame={onSectionNavigate}
            renderTaskAttachmentThumbnails={renderTaskAttachmentThumbnails}
          />
        </div>
      </div>
    </div>
  );
}
