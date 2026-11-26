import React, { useContext } from "react";
import { MentionChip, PlanReference, type OpenPlan, type ResolvedChatPlanRef } from "./codeterm/LinkedText";
import { canonicalMentions } from "./codeterm/canonicalMentions";
import { referenceMarkers } from "./codeterm/referenceMarkers";
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

const DOMIOS_HTML_TAG = /<\/?domios(?=$|[\s/>])(?:[^"'<>]|"[^"]*"|'[^']*')*>/gi;

export function remarkReferenceMarkers() {
  interface Node { type: string; value?: string; children?: Node[] }
  function split(child: Node): Node[] {
    if (child.type !== "html" || !child.value || !/<\/?domios(?=$|[\s/>])/i.test(child.value)) return [walk(child)];
    // `<domios ...>` is never allowed HTML: lifting both valid and malformed tags to text lets rehypePlanRefs
    // render valid chips while incomplete or malformed tags stay inert instead of being stripped by rehype-sanitize.
    const ranges: Array<[number, number]> = referenceMarkers(child.value).map((marker) => [marker.start, marker.end]);
    for (const match of child.value.matchAll(DOMIOS_HTML_TAG)) {
      const start = match.index!;
      const end = start + match[0].length;
      if (!ranges.some(([a, b]) => start < b && end > a)) ranges.push([start, end]);
    }
    if (!ranges.length) return [child];
    ranges.sort((a, b) => a[0] - b[0]);
    const nodes: Node[] = [];
    let cursor = 0;
    for (const [start, end] of ranges) {
      if (start > cursor) nodes.push({ type: "html", value: child.value.slice(cursor, start) });
      nodes.push({ type: "text", value: child.value.slice(start, end) });
      cursor = end;
    }
    if (cursor < child.value.length) nodes.push({ type: "html", value: child.value.slice(cursor) });
    return nodes;
  }
  // Parsed trees are shared between renders, so this copies the paths it changes instead of mutating them.
  function walk(node: Node): Node {
    if (!node.children || ["code", "inlineCode", "link"].includes(node.type)) return node;
    let changed = false;
    const children = node.children.flatMap((child) => {
      const next = split(child);
      if (next.length !== 1 || next[0] !== child) changed = true;
      return next;
    });
    return changed ? { ...node, children } : node;
  }
  return (tree: Node) => walk(tree);
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
