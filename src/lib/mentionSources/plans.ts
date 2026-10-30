import { canonicalMentionText } from "../../../packages/chat-ui/src/codeterm/canonicalMentions";
import type { Plan } from "../../components/Canvas/types";
import { mentionMatch, type MentionSource } from "../composerMentions";
import { planInMentionScope } from "./scope";

export function planMentionSource(read: (query: string, context: import("../composerMentions").MentionContext) => readonly Plan[] | Promise<readonly Plan[]>): MentionSource {
  return {
    id: "plans", group: "Plans",
    async search(query, context) {
      const { scope, signal } = context;
      signal.throwIfAborted();
      return (await read(query, context)).filter((plan) => planInMentionScope(plan, scope)).map((plan) => ({
        kind: "plan" as const, id: `plans:${plan.id}`, label: plan.title, group: "Plans",
        description: `${plan.id} · ${plan.mode ?? plan.status}`,
        insertText: canonicalMentionText("plan", plan.title, plan.id),
        match: mentionMatch(query, [plan.id, plan.title]),
      }));
    },
  };
}
