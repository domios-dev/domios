import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { ExternalLink, MessageCircle, X } from "lucide-react";
import type { Tab } from "../../../lib/types";
import { AgentCircle } from "../presence/AgentCircle";
import { presenceColor } from "../presence/identity";
import type { Plan } from "../types";
import { orderPlansByActivity, selectDefaultPlanId } from "./boardModel";
import { resolveCompanionTab, type CompanionKind } from "./companionResolver";
import { resolvePlannerTabIdFromPlan } from "../../shared/resolvePlanPeerPair";
import { ActivityDots } from "../../shared/ActivityDots";

interface PlanCompanionBubbleProps {
  plans: Plan[];
  tabs: Record<string, Tab>;
  planId?: string;
  orchestratorTabId?: string;
  onOpenTab?: (tabId: string) => void;
  // chain; production wires the real TabSessionChat (no forked chat).
  renderChat: (args: { tabId: string; chatBackend: boolean }) => ReactNode;
}

function tabHasLiveAgent(tab: Tab | undefined): boolean {
  return !!tab && !tab.hidden && !!tab.agent_type;
}

const KIND_LABEL: Record<CompanionKind, string> = {
  planner: "Planner",
  orchestrator: "Orchestrator",
};

function statusText(tab: Tab | undefined): string {
  switch (tab?.agent_state?.state) {
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
  tabs,
  planId,
  orchestratorTabId,
  onOpenTab,
  renderChat,
}: PlanCompanionBubbleProps) {
  const [open, setOpen] = useState(false);

  const activePlan = useMemo(() => {
    const ordered = orderPlansByActivity(plans);
    const id = selectDefaultPlanId(ordered, { planId, orchestratorTabId, tabs, workspaceId: undefined });
    return ordered.find((p) => p.id === id) ?? null;
  }, [plans, planId, orchestratorTabId, tabs]);

  const resolution = useMemo(() => {
    if (!activePlan) return null;
    return resolveCompanionTab(activePlan, (tabId) => tabHasLiveAgent(tabs[tabId]));
  }, [activePlan, tabs]);

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

  const tab = tabs[resolution.tabId];
  const color = presenceColor(resolution.tabId);
  const kindLabel = KIND_LABEL[resolution.kind];
  const name = tab?.title?.trim() || kindLabel;
  const working = tab?.agent_state?.state === "working";
  const chatBackend = tab?.backing?.kind === "chat_backend";
  const ownerTabId = activePlan?.owner.tab_id ?? null;
  const plannerTabId = activePlan ? resolvePlannerTabIdFromPlan(activePlan) : null;
  const orchestratorName =
    ownerTabId && tabs[ownerTabId]?.title?.trim()
      ? tabs[ownerTabId]!.title!.trim()
      : "Orchestrator";
  const plannerName =
    plannerTabId && tabs[plannerTabId]?.title?.trim()
      ? tabs[plannerTabId]!.title!.trim()
      : "Planner";
  const peerPairTitle =
    ownerTabId && plannerTabId
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
              agent={{ id: resolution.tabId, label: name, role: kindLabel, color }}
              size={20}
              live={working}
            />
            <div className="min-w-0 flex-1 leading-tight">
              <div className="truncate text-[13px] font-semibold" style={{ color: "var(--ct-fg)" }}>
                {name}
              </div>
              <div className="truncate text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                {kindLabel} · {statusText(tab)}
              </div>
            </div>
            {onOpenTab && (
              <button
                type="button"
                data-testid="plan-companion-open-tab"
                onClick={() => onOpenTab(resolution.tabId)}
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
            {renderChat({ tabId: resolution.tabId, chatBackend })}
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
        {working ? (
          <ActivityDots
            data-testid="plan-companion-presence"
            data-live="true"
            size="micro"
            tone="muted"
            active
            className="plan-companion-presence"
            style={{ color }}
          />
        ) : (
          <span
            data-testid="plan-companion-presence"
            data-live="false"
            className="plan-companion-presence"
            style={{ background: color }}
          />
        )}
      </button>
    </div>
  );
}
