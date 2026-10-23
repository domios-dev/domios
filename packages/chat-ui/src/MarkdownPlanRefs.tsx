import React, { useContext } from "react";
import { MentionChip, PlanReference, type OpenPlan, type ResolvedChatPlanRef } from "./codeterm/LinkedText";
import { canonicalMentions } from "./codeterm/canonicalMentions";
import { linkifyPlanRefs } from "./codeterm/planRefs";
import { splitAttachedPunctuation } from "./codeterm/attachedPunctuation";

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
  function mentionNodes(text: string): HastNode[] {
    const nodes: HastNode[] = [];
    let cursor = 0;
    for (const mention of canonicalMentions(text)) {
      nodes.push({ type: "text", value: text.slice(cursor, mention.start) });
      nodes.push({ type: "element", tagName: "span", properties: { "data-mention": JSON.stringify(mention) }, children: [{ type: "text", value: mention.text }] });
      cursor = mention.end;
    }
    nodes.push({ type: "text", value: text.slice(cursor) });
    return nodes;
  }
  function isRef(node: HastNode | undefined): boolean {
    return node?.type === "element" && (node.properties?.["data-mention"] !== undefined || node.properties?.["data-plan-id"] !== undefined);
  }
  function keepPunctuationAttached(nodes: HastNode[]): HastNode[] {
    const out: HastNode[] = [];
    for (const node of nodes.filter((candidate) => candidate.type !== "text" || candidate.value)) {
      const [attached, rest] = isRef(out[out.length - 1]) && node.type === "text" ? splitAttachedPunctuation(node.value ?? "") : ["", node.value ?? ""];
      if (attached) {
        const ref = out.pop() as HastNode;
        out.push({ type: "element", tagName: "span", properties: { className: ["whitespace-nowrap"] }, children: [ref, { type: "text", value: attached }] });
        if (rest) out.push({ type: "text", value: rest });
      } else out.push(node);
    }
    return out;
  }
  function visit(node: HastNode): void {
    if (!node.children || ["a", "code", "pre", "script", "style"].includes(node.tagName ?? "")) return;
    node.children = node.children.flatMap((child) => {
      if (child.type !== "text" || !child.value) {
        visit(child);
        return [child];
      }
      return keepPunctuationAttached(linkifyPlanRefs(child.value).flatMap((segment): HastNode | HastNode[] => segment.kind === "text"
        ? mentionNodes(segment.value)
        : { type: "element", tagName: "span", properties: { "data-plan-id": segment.planId, "data-target-id": segment.targetId, "data-target-kind": segment.targetKind }, children: [{ type: "text", value: segment.value }] }));
    });
  }
  return visit;
}

export function MarkdownPlanRef({ node, children, ...props }: React.ComponentProps<"span"> & { node?: { properties?: Record<string, unknown> } }) {
  const links = useContext(MarkdownPlanLinksContext);
  const mention = node?.properties?.["data-mention"];
  if (typeof mention === "string") return <MentionChip mention={JSON.parse(mention)} />;
  const planId = node?.properties?.["data-plan-id"];
  const targetId = node?.properties?.["data-target-id"];
  const kind = node?.properties?.["data-target-kind"];
  if (typeof planId === "string" && (kind === "plan" || kind === "task" || kind === "section")) {
    return <PlanReference segment={{ kind: "plan_ref", value: React.Children.toArray(children).join(""), planId, targetId: typeof targetId === "string" ? targetId : undefined, targetKind: kind }} {...links} />;
  }
  return <span {...props}>{children}</span>;
}
