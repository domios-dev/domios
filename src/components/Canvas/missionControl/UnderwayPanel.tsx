import { useId, useRef, useState } from "react";
import type { Tab, ApiConfig } from "../../../lib/types";
import { roleKind, roleKindLabel } from "../../../lib/roleKind";
import type { Plan } from "../types";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";
import { ActivityFeed } from "../../Supporter/ActivityFeed";
import { ActivityDots } from "../../shared/ActivityDots";
import { AnchoredPopover } from "../../shared/AnchoredPopover";
import { ChecklistItemDetail } from "../../Sidebar/ChecklistItemDetail";
import { checklistStateLabel, type AgentChecklistItem } from "../../Sidebar/agentChecklistModel";
import { useAgentChecklists, type ChecklistView } from "../../Sidebar/useAgentChecklists";
import { BoardPanel } from "./BoardPanel";
import type { BoardRefs } from "./BoardRefText";
import { checklistProgress, underwayRows } from "./underwayModel";
import { useUnderwayResize } from "./useUnderwayResize";

function Segment({ item, index, total, plans, tabId }: { item: AgentChecklistItem; index: number; total: number; plans: readonly Plan[]; tabId: string }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const id = useId();
  return <li>
    <button ref={anchor} type="button" className="mc-underway-segment" data-state={item.state}
      aria-label={`${index + 1} of ${total}: ${item.title}, ${checklistStateLabel(item.state)}`}
      aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen((value) => !value)}
      onKeyDown={(event) => {
        if (event.code !== "ArrowLeft" && event.code !== "ArrowRight") return;
        event.preventDefault();
        const buttons = Array.from(event.currentTarget.closest("ol")?.querySelectorAll("button") ?? []);
        buttons[(index + (event.code === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
      }}><span /></button>
    <AnchoredPopover open={open} anchor={anchor} placement="bottom-start" width={300} onClose={() => setOpen(false)}>
      <ChecklistItemDetail id={id} item={item} plans={plans} tabId={tabId} onClose={() => setOpen(false)} />
    </AnchoredPopover>
  </li>;
}

function UnderwayRow({ tab, task, view, plans, refs, deepLinks }: {
  tab: Tab; task: Plan["tasks"][number] | undefined; view: ChecklistView | undefined;
  plans: readonly Plan[]; refs: BoardRefs; deepLinks?: NeedsYouDeepLinks;
}) {
  const items = view?.status === "ready" ? view.checklist?.items ?? [] : [];
  const progress = checklistProgress(items);
  const workspaceId = tab.workspace_id ?? deepLinks?.workspaces.find((workspace) => workspace.tabs.includes(tab.id))?.id;
  return <li className="mc-underway-row">
    <span className="mc-underway-name"><button type="button" title={tab.title} disabled={!workspaceId || !deepLinks}
      onClick={() => { if (workspaceId) void deepLinks?.onSelectTab(workspaceId, tab.id); }}>{tab.title}</button>
      <ActivityDots size="micro" tone="muted" active={tab.agent_state?.state === "working"} /></span>
    <span className="mc-underway-kind" data-plain={roleKind(tab) === "worker"}>{roleKindLabel(tab)}</span>
    <button type="button" className="mc-underway-task" title={task?.name} disabled={!task || !refs.get(task.id)?.open}
      onClick={() => { if (task) refs.get(task.id)?.open?.(); }}>{task?.key ?? "–"}</button>
    <div className="mc-underway-track">
      {view?.status !== "ready" ? <span>{view?.status === "error" ? "Checklist unavailable" : "Loading checklist"}</span>
        : items.length === 0 ? <span>No checklist</span>
        : <ol aria-label={`Checklist of ${tab.title}`}>{items.map((item, index) => <Segment key={item.id} item={item} index={index} total={items.length} plans={plans} tabId={tab.id} />)}</ol>}
    </div>
    <span className="mc-underway-count">{view?.status === "ready" ? <><b>{progress.done}</b> / {progress.total}</> : "–"}</span>
  </li>;
}

export function UnderwayBoardRow({ api, teamId, plans, refs, deepLinks }: {
  api?: ApiConfig; teamId: string; plans: readonly Plan[]; refs: BoardRefs; deepLinks?: NeedsYouDeepLinks;
}) {
  const rows = underwayRows(Object.values(deepLinks?.tabs ?? {}), plans, teamId);
  const views = useAgentChecklists(rows.map(({ tab }) => tab.id));
  const { size, handle } = useUnderwayResize();
  return <div className="mc-board-live-row">
    {api && <div className="mc-reflexes-frame" style={{ height: size ? `${size.height}px` : undefined }}>
      <BoardPanel testId="mc-board-reflexes" title="Spinal-cord activity" className="mc-board-reflexes"><div className="mc-board-sub mc-reflexes-list"><ActivityFeed api={api} teamId={teamId} embedded /></div></BoardPanel>
      <button type="button" className="mc-underway-resize mc-underway-resize--right" aria-label="Resize activity and Underway; use arrow keys" title="Drag to resize" {...handle} />
    </div>}
    <div className="mc-underway-frame" style={{ flexBasis: size ? `${size.width}px` : undefined, height: size ? `${size.height}px` : undefined }}>
      <BoardPanel testId="mc-board-underway" title="Underway" meta={<span className="inline-flex items-center gap-1.5">{rows.length ? `${rows.length} with checklists` : "none"}<ActivityDots size="small" tone="muted" active={rows.some(({ tab }) => tab.agent_state?.state === "working")} /></span>}>
        <div className="mc-underway-list">
          {rows.length ? <ol aria-label="Working agents">{rows.map(({ tab, task }) => <UnderwayRow key={tab.id} tab={tab} task={task} view={views.get(tab.id)} plans={plans} refs={refs} deepLinks={deepLinks} />)}</ol>
            : <div className="mc-underway-empty"><p>No agent in this team has a checklist yet.</p><p>Agents appear here as soon as they keep one.</p></div>}
        </div>
        <button type="button" className="mc-underway-resize" aria-label="Resize Underway; use arrow keys to change width and height" title="Drag to resize" {...handle} />
      </BoardPanel>
    </div>
  </div>;
}
