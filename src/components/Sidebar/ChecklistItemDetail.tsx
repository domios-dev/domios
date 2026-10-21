import { usePlanTargetNavigation } from "../Canvas/teamViewNavigation";
import type { Plan } from "../Canvas/types";
import { checklistStateLabel, type AgentChecklistItem } from "./agentChecklistModel";

function ChecklistTaskLink({ target, label, onOpen }: { target: { planId: string; taskId: string }; label: string; onOpen: () => void }) {
  const navigate = usePlanTargetNavigation();
  return <button type="button" className="text-[var(--ct-accent)] text-left underline" onClick={() => { navigate(target); onOpen(); }}>{label}</button>;
}

export function ChecklistItemDetail({ item, plans, tabId, id, onClose }: { item: AgentChecklistItem; plans: readonly Plan[]; tabId: string; id?: string; onClose: () => void }) {
  const linked = item.link?.kind === "domios_task" ? item.link : null;
  const plan = plans.find((candidate) => linked ? candidate.id === linked.plan_id : item.locked && candidate.tasks.some((task) => task.assigned_tab_id === tabId));
  const task = plan?.tasks.find((candidate) => linked ? candidate.id === linked.task_id : candidate.assigned_tab_id === tabId);
  const taskTarget = linked ? { planId: linked.plan_id, taskId: linked.task_id } : plan && task ? { planId: plan.id, taskId: task.id } : null;
  return (
      <section id={id} role="region" aria-label="Checklist item details" className="ct-menu p-3 text-xs whitespace-normal break-words space-y-2"
        onClick={(event) => event.stopPropagation()}>
        <p className="font-medium">{item.title}</p>
        {item.description && <p>{item.description}</p>}
        <p>{checklistStateLabel(item.state)} · {item.locked ? "Task criterion (locked)" : "Agent item"}</p>
        {item.state === "in_progress" && item.activeForm && <p>{item.activeForm}</p>}
        {item.reason && <p>Reason: {item.reason}</p>}
        {item.link?.kind === "url" && <a className="text-[var(--ct-accent)] underline" href={item.link.url} target="_blank" rel="noopener noreferrer">{item.link.label} · {item.link.url}</a>}
        {taskTarget && <ChecklistTaskLink target={taskTarget} label={task ? `${task.key ?? task.id} · ${task.name}` : linked?.label ?? ""} onOpen={onClose} />}
      </section>
  );
}
