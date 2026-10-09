import { useEffect, useRef, type ReactNode } from "react";
import "../../../../src/components/Sidebar/sidebarPanel.css";
import { type CompactRailGroup, SidebarCompactRail } from "../../../../src/components/Sidebar/SidebarCompactRail";
import type { Tab, TabStatus } from "../../../../src/lib/types";
import { agentTab, CODEX, ProductProviders, shellTab, WORKSPACE_ID } from "../fixtures";
import { ForcePseudo } from "../stageTools";
import { defineSpecimen } from "../types";

const GENERAL = "gallery-general";
const working = { state: "working" as const, status_line: "Rebuilding the gallery stages… (4m 12s · ↓ 18.2k tokens)" };

const general = agentTab("rail-general", { title: "General", workspace_id: GENERAL, role: "Manager", role_profile: "general_agent", protected: true, description: "Planning the 1.14 release" });
const lead = agentTab("rail-lead", { title: "Motion Design Lead", role: "Manager", role_profile: "orchestrator", can_spawn: true, agent_state: working });
const planner = agentTab("rail-planner", { title: "Release planner", role: "Manager", role_profile: "planner", agent_state: { state: "question", subtype: "permission" } });
const triage = agentTab("rail-triage", { title: "Nightly triage", agent_type: CODEX.id, agent_session_model: "gpt-6-sol", boot_stage: "spawn_failed", boot_failure_reason: "provider exited with code 1" });
const docs = agentTab("rail-docs", { title: "Docs sync" });
const shell = shellTab("rail-shell", { title: "zsh" });

const STATUSES: Record<string, TabStatus> = { [lead.id]: "Working", [planner.id]: "Waiting", [triage.id]: "Error" };

const GROUPS: CompactRailGroup[] = [
  { workspaceId: GENERAL, label: "General", tabs: [general], activeTabId: null },
  { workspaceId: WORKSPACE_ID, label: "CodeTerm", tabs: [lead, planner, triage, docs, shell], activeTabId: docs.id },
];

/** Hovers a rail button so the real peek opens, then maps its viewport coordinates into the transformed stage. */
function HoverPeek({ tab, children }: { tab: Tab; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const stage = ref.current?.closest<HTMLElement>(".gallery-stage-instance");
    if (!stage) return;
    const place = () => {
      const peek = stage.querySelector<HTMLElement>(".ct-compact-peek:not([data-gallery-placed])");
      if (!peek) return;
      const box = stage.getBoundingClientRect();
      peek.dataset.galleryPlaced = "true";
      peek.style.top = `${Number.parseFloat(peek.style.top) - box.top}px`;
      peek.style.left = `${Number.parseFloat(peek.style.left) - box.left}px`;
    };
    const observer = new MutationObserver(place);
    observer.observe(stage, { childList: true, subtree: true });
    const timer = window.setTimeout(() => {
      ref.current?.querySelector(`[aria-label="Open tab ${tab.title}"]`)?.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, relatedTarget: null }));
    }, 50);
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [tab]);
  return <div ref={ref}><ForcePseudo selector={`[aria-label="Open tab ${tab.title}"]`} state="hover">{children}</ForcePseudo></div>;
}

type RailSpecimenProps = { groups: CompactRailGroup[]; hover?: Tab };

function RailSpecimen({ groups, hover }: RailSpecimenProps) {
  const rail = <div className="gallery-rail"><SidebarCompactRail groups={groups} tabStatuses={STATUSES} onActivate={() => undefined} /></div>;
  return <ProductProviders tabs={groups.flatMap((group) => group.tabs)}>
    <div className="gallery-rail-stage">{hover ? <HoverPeek tab={hover}>{rail}</HoverPeek> : rail}</div>
  </ProductProviders>;
}

export default defineSpecimen({
  id: "sidebar-compact-rail",
  title: "Sidebar compact rail",
  category: "Sidebar",
  where: "src/components/Sidebar/SidebarCompactRail.tsx",
  stage: "wide",
  notes: "The shipped compact rail: the General group above the workspace's main agents, monogram labels, the active tab ring and working / waiting / failed status dots from tabActivity. Flyout stages hover a rail button so the real TabItem peek opens beside it.",
  Component: RailSpecimen,
  states: [
    { id: "groups", label: "General + workspace · active · status dots", props: { groups: GROUPS } },
    { id: "flyout-working", label: "Hover flyout · working orchestrator", props: { groups: GROUPS, hover: lead } },
    { id: "flyout-waiting", label: "Hover flyout · waiting on you", props: { groups: GROUPS, hover: planner } },
    { id: "flyout-failed", label: "Hover flyout · launch failed", props: { groups: GROUPS, hover: triage } },
  ],
});
