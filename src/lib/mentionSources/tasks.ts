import { referenceMarkerText } from "../../../packages/chat-ui/src/codeterm/referenceMarkers";
import type { PlanView as Plan } from "../../components/Canvas/planProjection";
import { mentionMatch, type MentionSource } from "../composerMentions";
import { planInMentionScope } from "./scope";

const STATUS_ORDER = ["in_progress", "in_review", "blocked", "pending", "paused", "done", "cancelled"];

export function taskMentionSource(read: (query: string, context: import("../composerMentions").MentionContext) => readonly Plan[] | Promise<readonly Plan[]>): MentionSource {
  return {
    id: "tasks", group: "Tasks",
    async search(query, context) {
      const { scope, signal } = context;
      signal.throwIfAborted();
      return (await read(query, context)).filter((plan) => planInMentionScope(plan, scope)).flatMap((plan) => plan.tasks.map((task) => ({
        kind: "task" as const, status: task.status, id: `tasks:${task.id}`, label: task.key ?? task.id, group: "Tasks",
        description: `${task.name} · ${task.status} · ${plan.title}`,
        insertText: referenceMarkerText("task", task.id, plan.id),
        match: mentionMatch(query, [task.key ?? task.id, task.name]),
        exact: !!task.key && task.key.toLowerCase() === query.toLowerCase(),
        statusOrder: STATUS_ORDER.indexOf(task.status),
      })));
    },
  };
}
