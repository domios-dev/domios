import type { Plan } from "../../components/Canvas/types";
import { mentionMatch, type MentionSource } from "../composerMentions";
import { planInMentionScope } from "./scope";

export function planMentionSource(read: () => readonly Plan[]): MentionSource {
  return {
    id: "plans", group: "Plans",
    async search(query, { scope, signal }) {
      signal.throwIfAborted();
      return read().filter((plan) => planInMentionScope(plan, scope)).map((plan) => ({
        id: `plans:${plan.id}`, label: plan.title, group: "Plans",
        description: `${plan.id} · ${plan.mode ?? plan.status}`,
        insertText: `@${plan.title} (plan ${plan.id})`,
        match: mentionMatch(query, [plan.id, plan.title]),
      }));
    },
  };
}
