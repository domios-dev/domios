import { useCallback, useEffect, useRef } from "react";
import { X } from "lucide-react";
import { DetailContent } from "../canvasRender/DetailContent";
import type { Pane } from "../../../lib/types";
import type { Plan, Section, Task } from "../types";
import type { SectionNavArgs } from "../nodeRegistry";

export type PlanDetailKind = "task" | "plan" | "spec";

export interface PlanDetailOverlayProps {
  kind: PlanDetailKind;
  task?: Task;
  plan?: Plan;
  spec?: Plan;
  specSections?: Section[];
  planId?: string;
  panes?: Record<string, Pane>;
  onOpenPane?: (paneId: string) => void;
  onClose: () => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  /** Preserves canvas Playwright hooks when set to `canvas-detail`. */
  testIdPrefix?: string;
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
  panes,
  onOpenPane,
  onClose,
  onSectionClick,
  onSectionFrame,
  testIdPrefix = "plan-detail",
}: PlanDetailOverlayProps) {
  const panelRef = useRef<HTMLDivElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const tid = (suffix: string) => `${testIdPrefix}-${suffix}`;

  const onKeyDown = useCallback(
    (e: DetailKeyEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
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
    [onClose],
  );

  useEffect(() => {
    returnFocusRef.current = (document.activeElement as HTMLElement | null) ?? null;
    const id = window.setTimeout(() => panelRef.current?.focus(), 0);
    return () => {
      window.clearTimeout(id);
      returnFocusRef.current?.focus?.();
    };
  }, [kind, task?.id, plan?.id, spec?.id]);

  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    const listener = (event: KeyboardEvent) => onKeyDown(event);
    panel.addEventListener("keydown", listener);
    return () => panel.removeEventListener("keydown", listener);
  }, [onKeyDown]);

  const title = detailTitle(kind, task, plan, spec);
  const navPlanId = planId ?? plan?.id ?? spec?.id;

  return (
    <div
      data-testid={tid("overlay")}
      role="dialog"
      aria-modal="true"
      aria-label={`${title} — detail`}
      onKeyDown={onKeyDown}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 40,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: "clamp(12px, 4vh, 56px) clamp(12px, 4vw, 64px)",
        background: "color-mix(in srgb, var(--ct-bg) 62%, transparent)",
        backdropFilter: "blur(3px)",
      }}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        data-testid={tid("panel")}
        style={{
          position: "relative",
          width: "100%",
          maxWidth: kind === "task" ? 900 : 1180,
          height: "100%",
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          borderRadius: "var(--ct-radius-lg)",
          border: "1px solid var(--ct-border-default)",
          background: "var(--ct-bg)",
          boxShadow: "0 24px 80px -20px color-mix(in srgb, var(--ct-shadow, #000) 65%, transparent)",
          overflow: "hidden",
          outline: "none",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px 10px 16px", borderBottom: "1px solid var(--ct-border-subtle)", flexShrink: 0 }}>
          <span data-testid={tid("title")} style={{ flex: 1, minWidth: 0, fontFamily: "var(--ct-font-ui)", fontWeight: 700, fontSize: "var(--ct-font-size-md, 15px)", color: "var(--ct-fg)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {title}
          </span>
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
        <div data-testid={tid("body")} style={{ position: "relative", flex: 1, minHeight: 0, overflowY: "auto" }}>
          <DetailContent
            kind={kind}
            planId={navPlanId}
            task={task}
            plan={plan}
            spec={spec}
            specSections={specSections}
            panes={panes}
            onOpenPane={onOpenPane}
            onSectionClick={onSectionClick}
            onSectionFrame={(a) => { onClose(); onSectionFrame?.(a); }}
          />
        </div>
      </div>
    </div>
  );
}
