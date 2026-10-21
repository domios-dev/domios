import { useState } from "react";
import { invoke } from "../../lib/bridge";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useMuxSelector } from "../../store/multiplexer";
import { boundedChecklist, type AgentChecklist, type AgentChecklistItem } from "./agentChecklistModel";
import { useAgentChecklists } from "./useAgentChecklists";

export function ChecklistProposalCard({ checklist, title, onReviewed }: {
  checklist: AgentChecklist; title: string; onReviewed?: (checklist: AgentChecklist) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [items, setItems] = useState(checklist.items);
  const [editBase, setEditBase] = useState(checklist);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reviewed, setReviewed] = useState(false);
  const save = async (amend: boolean) => {
    setBusy(true); setError(null);
    try {
      const result = await invoke("tab_todo_review", { tabId: checklist.tabId, request: {
        generation: (amend ? editBase : checklist).generation, revision: (amend ? editBase : checklist).revision,
        ...(amend ? { items: items.map((item) => ({ ...(item.id ? { id: item.id } : {}), content: item.title, state: item.state, ...(item.reason ? { reason: item.reason } : {}) })) } : {}),
        ...(note.trim() ? { note: note.trim() } : {}),
      } });
      const updated = boundedChecklist(result);
      if (!updated) throw new Error("Checklist review returned no checklist");
      setReviewed(true); onReviewed?.(updated);
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };
  if (reviewed) return null;
  const editTitle = (index: number, title: string) => setItems(items.map((item, at) => at === index ? { ...item, title } : item));
  return <section aria-label={`Checklist proposal from ${title}`} className="m-2 rounded border border-dashed border-[var(--ct-border)] p-3 text-xs">
    <strong>{title} shared a checklist</strong><span className="ml-2 text-[var(--ct-muted-fg)]">Not reviewed yet · work continues</span>
    <ol className="my-2 space-y-1">{(editing ? items : checklist.items).map((item, index) => <li key={item.id || `new-${index}`} className="flex items-center gap-2">
      <span className="text-[var(--ct-muted-fg)]">{item.locked ? "Criterion" : "Plan"}</span>
      {editing && !item.locked ? <><input aria-label={`Plan item ${index + 1}`} maxLength={120} value={item.title} onChange={(event) => editTitle(index, event.target.value)} />
        <button type="button" disabled={busy} aria-label={`Remove ${item.title}`} onClick={() => setItems(items.filter((_, at) => at !== index))}>Remove</button></> : <span>{item.title}</span>}
    </li>)}</ol>
    {editing && <><button type="button" disabled={busy || items.length >= 12} onClick={() => setItems([...items, { id: "", title: "", state: "open" } satisfies AgentChecklistItem])}>Add item</button>
      <label className="block">Note<input aria-label="Review note" maxLength={512} value={note} onChange={(event) => setNote(event.target.value)} /></label></>}
    {error && <InlineErrorNotice error={error} operation="saving the checklist" role="alert" />}
    <div className="mt-2 flex gap-2"><button type="button" disabled={busy} onClick={() => void save(editing)}>{editing ? "Save amendment" : "Approve"}</button>
      <button type="button" disabled={busy} onClick={() => { setEditing(!editing); setItems(checklist.items); setEditBase(checklist); }}>{editing ? "Cancel" : "Edit"}</button></div>
  </section>;
}

export function ChecklistProposals({ parentTabId }: { parentTabId: string }) {
  const tabs = useMuxSelector((state) => state.tabs);
  const children = Object.values(tabs).filter((tab) => tab.parent_tab_id === parentTabId && tab.has_checklist);
  const views = useAgentChecklists(children.map((tab) => tab.id));
  return <div className="max-h-72 shrink-0 overflow-y-auto">{children.map((tab) => {
    const view = views.get(tab.id);
    const checklist = view?.status === "ready" ? view.checklist : null;
    return checklist?.proposal?.state === "proposed" ? <ChecklistProposalCard key={`${tab.id}:${checklist.generation}`} checklist={checklist} title={tab.title} /> : null;
  })}</div>;
}
