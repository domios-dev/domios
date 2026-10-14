import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ArrowDownToLine } from "lucide-react";
import { useUpdatePanel } from "../../hooks/useUpdatePanel";
import { UpdatePanel } from "../UpdatePanel";
import { BusySpinner } from "../shared/BusyState";

export function UpdateTray() {
  const update = useUpdatePanel();
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    panel.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const close = (event: MouseEvent) => {
      if (!panel.current?.contains(event.target as Node) && !trigger.current?.contains(event.target as Node)) setOpen(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") { setOpen(false); trigger.current?.focus(); }
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", key);
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", key); };
  }, [open]);
  const available = update.items.length > 0;
  if (!available && !update.busy && !update.error && !open) return null;
  const rect = trigger.current?.getBoundingClientRect();
  return <>
    <button ref={trigger} type="button" data-testid="settings-update-action" aria-label="Update" aria-haspopup="dialog" aria-expanded={open}
      onClick={() => setOpen(!open)} title={update.error ? "Update failed" : "Updates available"}
      className={`group inline-flex h-6 items-center gap-1 rounded-full ${update.error ? "bg-[var(--ct-red)]" : "bg-[var(--ct-accent)]"} pl-1.5 pr-2 text-[11px] font-medium text-[var(--ct-on-accent)] hover:brightness-110`}>
      {update.busy ? <BusySpinner size={12} /> : <ArrowDownToLine size={12} />}
      <span>Update</span><span role="status" className="hidden sm:inline">{update.error ? "failed" : update.busy ? "updating" : available ? "updates available" : "up to date"}</span>
    </button>
    {open && createPortal(<div ref={panel} role="dialog" aria-label="Updates" className="ct-portal-menu fixed z-[70] w-[min(380px,calc(100vw-16px))]"
      style={{ top: rect ? rect.bottom + 4 : 36, right: rect ? Math.max(8, window.innerWidth - rect.right) : 8 }}>
      <UpdatePanel />
    </div>, document.body)}
  </>;
}
