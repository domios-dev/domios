import { useState } from "react";
import { Pencil } from "lucide-react";
import { UI_BUTTON } from "../shared/design";

export function MachineName({ name, onRename, disabledReason }: {
  name: string;
  onRename?: (name: string) => Promise<boolean>;
  disabledReason?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(name);
  const [saving, setSaving] = useState(false);
  const [failed, setFailed] = useState(false);
  async function save() {
    const next = draft.trim();
    if (!next || saving || !onRename) return;
    if (next === name) { setEditing(false); return; }
    setSaving(true);
    setFailed(false);
    try {
      if (await onRename(next)) setEditing(false);
      else setFailed(true);
    } catch {
      setFailed(true);
    } finally {
      setSaving(false);
    }
  }
  return <span className="group/name flex min-w-0 flex-1 items-center gap-1" onClick={(event) => event.stopPropagation()} onKeyDown={(event) => event.stopPropagation()}>
    {editing ? <input aria-label={`Name for ${name}`} value={draft} autoFocus disabled={saving} aria-busy={saving}
      className="min-w-0 w-full rounded border border-[var(--ct-accent)] bg-[var(--ct-bg)] px-1 text-[var(--ct-fg)]"
      onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => {
        if (event.code === "Escape") { event.preventDefault(); setEditing(false); }
        if (event.code === "Enter") { event.preventDefault(); void save(); }
      }} /> : <>
      <span className="truncate">{name}</span>
      {(onRename || disabledReason) && <button type="button" aria-label={`Rename ${name}`} title={disabledReason ?? `Rename ${name}`}
        disabled={!!disabledReason} onClick={() => { setDraft(name); setEditing(true); }}
        className={`${UI_BUTTON.icon} shrink-0 opacity-0 group-hover/name:opacity-100 group-focus-within/name:opacity-100`}>
        <Pencil size={12} aria-hidden="true" />
      </button>}
    </>}
    {failed && editing && <span role="alert" className="text-[11px] text-[var(--ct-red)]">Could not rename. Try again.</span>}
  </span>;
}
