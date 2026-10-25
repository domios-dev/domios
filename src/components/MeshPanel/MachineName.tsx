import { Pencil } from "lucide-react";
import { UI_BUTTON } from "../shared/design";
import { useMachineName } from "./useMachineName";

export function MachineName({ name, onRename, disabledReason }: {
  name: string;
  onRename?: (name: string) => Promise<boolean>;
  disabledReason?: string;
}) {
  const { editing, draft, setDraft, saving, failed, begin, cancel, save } = useMachineName(name, onRename);
  return <span className="group/name flex min-w-0 flex-1 items-center gap-1" onClick={(event) => event.stopPropagation()} onKeyDown={(event) => event.stopPropagation()}>
    {editing ? <><input aria-label={`Name for ${name}`} value={draft} autoFocus disabled={saving} aria-busy={saving}
      className="min-w-0 w-full rounded border border-[var(--ct-accent)] bg-[var(--ct-bg)] px-1 text-[var(--ct-fg)]"
      onBlur={() => void save()} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => {
        if (event.code === "Escape") { event.preventDefault(); cancel(); }
        if (event.code === "Enter") { event.preventDefault(); void save(); }
      }} /><button type="button" disabled={saving || !draft.trim()} onMouseDown={(event) => event.preventDefault()} onClick={() => void save()} className={UI_BUTTON.compact}>Save</button></> : <>
      <span className="truncate">{name}</span>
      {(onRename || disabledReason) && <button type="button" aria-label={`Rename ${name}`} title={disabledReason ?? `Rename ${name}`}
        disabled={!!disabledReason} onClick={begin}
        className={`${UI_BUTTON.icon} shrink-0 opacity-0 group-hover/name:opacity-100 group-focus-within/name:opacity-100`}>
        <Pencil size={12} aria-hidden="true" />
      </button>}
    </>}
    {failed && editing && <span role="alert" className="text-[11px] text-[var(--ct-red)]">{failed}</span>}
  </span>;
}
