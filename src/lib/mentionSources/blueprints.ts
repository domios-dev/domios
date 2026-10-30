import { canonicalMentionText } from "../../../packages/chat-ui/src/codeterm/canonicalMentions";
import type { AutomationGraph, AutomationNode } from "../../components/Automation/types";
import { mentionMatch, type MentionContext, type MentionSource } from "../composerMentions";
import type { Tab } from "../types";

export function blueprintInMentionScope(node: AutomationNode, { scope }: Pick<MentionContext, "scope">, tabs: Record<string, Tab>): boolean {
  const identity = node.identity;
  const owner = identity.owner_tab_id ? tabs[identity.owner_tab_id] : null;
  const teamId = identity.team_id ?? owner?.team_id ?? null;
  return identity.kind === "blueprint" && identity.workspace_id === scope.workspaceId
    && (scope.teamId ? teamId === scope.teamId || !teamId : !teamId)
    && (!identity.owner_tab_id || !!owner);
}

export function blueprintMentionSource(read: (ctx: MentionContext) => Promise<AutomationGraph>, tabs: () => Record<string, Tab>): MentionSource {
  return {
    id: "blueprints", group: "Blueprints",
    async search(query, ctx) {
      if (!ctx.scope.workspaceId) return [];
      const graph = await read(ctx);
      ctx.signal.throwIfAborted();
      return graph.nodes.filter((node) => blueprintInMentionScope(node, ctx, tabs())).map((node) => ({
        kind: "blueprint" as const, id: `blueprints:${node.identity.id}`, label: node.name, group: "Blueprints",
        description: `${node.identity.id} · ${node.state ?? "idle"} · ${node.purpose ?? ""}`,
        insertText: canonicalMentionText("blueprint", node.name, node.identity.id),
        match: mentionMatch(query, [node.identity.id, node.name, node.purpose ?? ""]),
      }));
    },
  };
}
