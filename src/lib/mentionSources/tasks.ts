import type { Plan } from "../../components/Canvas/types";
import { mentionMatch, type MentionSource } from "../composerMentions";
import { planInMentionScope } from "./scope";

const STATUS_ORDER = ["in_progress", "in_review", "blocked", "pending", "paused", "done", "cancelled"];

export function taskMentionSource(read: () => readonly Plan[]): MentionSource {
  return {
    id: "tasks", group: "Tasks",
    async search(query, { scope, signal }) {
      signal.throwIfAborted();
      return read().filter((plan) => planInMentionScope(plan, scope)).flatMap((plan) => plan.tasks.map((task) => ({
        id: `tasks:${task.id}`, label: task.key ?? task.id, group: "Tasks",
        description: `${task.name} · ${task.status} · ${plan.title}`,
        insertText: task.key ?? task.id,
        match: mentionMatch(query, [task.key ?? task.id, task.name]),
        exact: !!task.key && task.key.toLowerCase() === query.toLowerCase(),
        statusOrder: STATUS_ORDER.indexOf(task.status),
      })));
    },
  };
}
