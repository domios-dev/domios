import React, { useContext } from "react";
import { PlanReference, type OpenPlan, type ResolvedChatPlanRef } from "./codeterm/LinkedText";
import { linkifyPlanRefs } from "./codeterm/planRefs";

export interface MarkdownPlanLinks {
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, targetId?: string, targetKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
}

export const MarkdownPlanLinksContext = React.createContext<MarkdownPlanLinks>({});

interface HastNode {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
}

export function rehypePlanRefs() {
  function visit(node: HastNode): void {
    if (!node.children || ["a", "code", "pre", "script", "style"].includes(node.tagName ?? "")) return;
    node.children = node.children.flatMap((child) => {
      if (child.type !== "text" || !child.value) {
        visit(child);
        return [child];
      }
      return linkifyPlanRefs(child.value).map((segment): HastNode => segment.kind === "text"
        ? { type: "text", value: segment.value }
        : { type: "element", tagName: "span", properties: { "data-plan-id": segment.planId, "data-target-id": segment.targetId, "data-target-kind": segment.targetKind }, children: [{ type: "text", value: segment.value }] });
    });
  }
  return visit;
}

export function MarkdownPlanRef({ node, children, ...props }: React.ComponentProps<"span"> & { node?: { properties?: Record<string, unknown> } }) {
  const links = useContext(MarkdownPlanLinksContext);
  const planId = node?.properties?.["data-plan-id"];
  const targetId = node?.properties?.["data-target-id"];
  const kind = node?.properties?.["data-target-kind"];
  if (typeof planId === "string" && (kind === "plan" || kind === "task" || kind === "section")) {
    return <PlanReference segment={{ kind: "plan_ref", value: React.Children.toArray(children).join(""), planId, targetId: typeof targetId === "string" ? targetId : undefined, targetKind: kind }} {...links} />;
  }
  return <span {...props}>{children}</span>;
}
