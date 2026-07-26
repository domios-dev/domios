import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { ExternalLink, MessageCircle, X } from "lucide-react";
import type { Pane } from "../../../lib/types";
import { AgentCircle } from "../presence/AgentCircle";
import { presenceColor } from "../presence/identity";
import type { Plan } from "../types";
import { orderPlansByActivity, selectDefaultPlanId } from "./boardModel";
import { resolveCompanionPane, type CompanionKind } from "./companionResolver";
import { resolvePlannerPaneIdFromPlan } from "../../shared/resolvePlanPeerPair";

interface PlanCompanionBubbleProps {
  plans: Plan[];
  panes: Record<string, Pane>;
  planId?: string;
  orchestratorPaneId?: string;
  onOpenPane?: (paneId: string) => void;
  // chain; production wires the real PaneSessionChat (no forked chat).
  renderChat: (args: { paneId: string; chatBackend: boolean }) => ReactNode;
}

function paneHasLiveAgent(pane: Pane | undefined): boolean {
  return !!pane && !pane.hidden && !!pane.agent_type;
}

const KIND_LABEL: Record<CompanionKind, string> = {
  planner: "Planner",
  orchestrator: "Orchestrator",
};

function statusText(pane: Pane | undefined): string {
  switch (pane?.agent_state?.state) {
    case "working":
      return "Working";
    case "question":
      return "Needs input";
    case "idle":
      return "Idle";
    default:
      return "Live";
  }
}

export function PlanCompanionBubble({
  plans,
  panes,
  planId,
  orchestratorPaneId,
  onOpenPane,
  renderChat,
}: PlanCompanionBubbleProps) {
  const [open, setOpen] = useState(false);

  const activePlan = useMemo(() => {
    const ordered = orderPlansByActivity(plans);
    const id = selectDefaultPlanId(ordered, { planId, orchestratorPaneId, panes, workspaceId: undefined });
    return ordered.find((p) => p.id === id) ?? null;
  }, [plans, planId, orchestratorPaneId, panes]);

  const resolution = useMemo(() => {
    if (!activePlan) return null;
    return resolveCompanionPane(activePlan, (paneId) => paneHasLiveAgent(panes[paneId]));
  }, [activePlan, panes]);

  useEffect(() => {
    if (!resolution) setOpen(false);
  }, [resolution]);

  const handleKey = useCallback((e: KeyboardEvent) => {
    if (e.key === "Escape") setOpen(false);
  }, []);

  useEffect(() => {
    if (!open) return;
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [open, handleKey]);

  if (!resolution) return null;

  const pane = panes[resolution.paneId];
  const color = presenceColor(resolution.paneId);
  const kindLabel = KIND_LABEL[resolution.kind];
  const name = pane?.title?.trim() || kindLabel;
  const working = pane?.agent_state?.state === "working";
  const chatBackend = pane?.backing?.kind === "chat_backend";
  const ownerPaneId = activePlan?.owner.pane_id ?? null;
  const plannerPaneId = activePlan ? resolvePlannerPaneIdFromPlan(activePlan) : null;
  const orchestratorName =
    ownerPaneId && panes[ownerPaneId]?.title?.trim()
      ? panes[ownerPaneId]!.title!.trim()
      : "Orchestrator";
  const plannerName =
    plannerPaneId && panes[plannerPaneId]?.title?.trim()
      ? panes[plannerPaneId]!.title!.trim()
      : "Planner";
  const peerPairTitle =
    ownerPaneId && plannerPaneId
      ? `${orchestratorName} ⟷ ${plannerName}`
      : null;
  const bubbleTitle = peerPairTitle ?? `${name} · ${kindLabel}`;

  return (
    <div
      data-testid="plan-companion"
      className="pointer-events-none absolute bottom-4 right-4 z-40 flex flex-col items-end gap-2"
    >
      {open && (
        <div
          data-testid="plan-companion-popover"
          role="dialog"
          aria-label={`${kindLabel} chat`}
          className="pointer-events-auto flex flex-col overflow-hidden rounded-xl border shadow-2xl"
          style={{
            width: 380,
            height: "min(60vh, 640px)",
            maxWidth: "calc(100vw - 2rem)",
            background: "var(--ct-bg)",
            borderColor: "var(--ct-border)",
          }}
        >
          <header
            className="flex shrink-0 items-center gap-2 border-b px-3 py-2"
            style={{ borderColor: "var(--ct-border)", background: "var(--ct-titlebar-bg)" }}
          >
            <AgentCircle
              agent={{ id: resolution.paneId, label: name, role: kindLabel, color }}
              size={20}
              live={working}
            />
            <div className="min-w-0 flex-1 leading-tight">
              <div className="truncate text-[13px] font-semibold" style={{ color: "var(--ct-fg)" }}>
                {name}
              </div>
              <div className="truncate text-[11px]" style={{ color: "var(--ct-fg-muted)" }}>
                {kindLabel} · {statusText(pane)}
              </div>
            </div>
            {onOpenPane && (
              <button
                type="button"
                data-testid="plan-companion-open-pane"
                onClick={() => onOpenPane(resolution.paneId)}
                title="Open full tab"
                className="flex h-6 w-6 shrink-0 items-center justify-center rounded border cursor-pointer"
                style={{ background: "var(--ct-bg)", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
              >
                <ExternalLink size={13} />
              </button>
            )}
            <button
              type="button"
              data-testid="plan-companion-close"
              onClick={() => setOpen(false)}
              title="Close"
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded border cursor-pointer"
              style={{ background: "var(--ct-bg)", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
            >
              <X size={13} />
            </button>
          </header>
          <div className="min-h-0 flex-1">
            {renderChat({ paneId: resolution.paneId, chatBackend })}
          </div>
        </div>
      )}
      <button
        type="button"
        data-testid="plan-companion-bubble"
        data-companion-kind={resolution.kind}
        aria-label={peerPairTitle ? `Talk to orchestration group (${peerPairTitle})` : `Talk to ${kindLabel}`}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        title={bubbleTitle}
        className="pointer-events-auto relative flex items-center justify-center rounded-full border shadow-lg cursor-pointer transition-transform hover:scale-105"
        style={{
          width: 48,
          height: 48,
          background: "var(--ct-bg)",
          borderColor: "var(--ct-border)",
        }}
      >
        <MessageCircle size={22} strokeWidth={2} aria-hidden style={{ color: "var(--ct-fg)" }} />
        <span
          data-testid="plan-companion-presence"
          data-live={working ? "true" : "false"}
          className={`plan-companion-presence${working ? " is-live" : ""}`}
          style={{ background: color }}
        />
      </button>
    </div>
  );
}
