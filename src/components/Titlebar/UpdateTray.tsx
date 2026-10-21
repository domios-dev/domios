import { useCallback, useEffect, useRef, useState } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { ArrowDownToLine } from "lucide-react";
import { useUpdatePanel } from "../../hooks/useUpdatePanel";
import { UpdatePanel } from "../UpdatePanel";
import { BusySpinner } from "../shared/BusyState";

export function UpdateTray() {
  const update = useUpdatePanel();
  const [open, setOpen] = useState(false);
  const [quietError, setQuietError] = useState<string | null>(null);
  useEffect(() => {
    if (!update.error) { setQuietError(null); return; }
    const expiry = window.setTimeout(() => setQuietError(update.error ?? null), 10_000);
    return () => window.clearTimeout(expiry);
  }, [update.error]);
  const urgent = Boolean(update.error && quietError !== update.error);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useEffect(() => {
    if (open) panel.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [open]);
  const offers = update.items.some(item => !item.channelError);
  const quiet = !update.busy && (Boolean(update.error) || !offers);
  const available = update.items.length > 0 || update.rows.some(row => row.state === "completed" || row.state === "failed");
  if (!available && !update.busy && !update.error && !open) return null;
  return <>
    <button ref={trigger} type="button" data-testid="settings-update-action" aria-label="Update" aria-haspopup="dialog" aria-expanded={open}
      onClick={() => { setOpen(!open); setQuietError(update.error ?? null); }} title={update.error ? "Update failed" : offers ? "Updates available" : "Update history"}
      className={`group inline-flex h-6 items-center gap-1 rounded-full ${urgent ? "bg-[var(--ct-red)] text-[var(--ct-on-accent)]" : quiet ? "bg-[var(--ct-surface)] text-[var(--ct-muted-fg)]" : "bg-[var(--ct-accent)] text-[var(--ct-on-accent)]"} pl-1.5 pr-2 text-[11px] font-medium hover:brightness-110`}>
      {update.busy ? <BusySpinner size={12} /> : <ArrowDownToLine size={12} />}
      <span role="status" className="max-[430px]:hidden">{urgent ? "Update failed" : update.busy ? "Updating" : quiet ? "Updates" : "Update"}</span>
    </button>
    <AnchoredPopover open={open} anchor={trigger} placement="bottom-end" onClose={close}>
      <div ref={panel} role="dialog" aria-label="Updates" className="w-[400px] max-w-[calc(100vw-24px)] max-h-[min(640px,80vh)] overflow-y-auto">
        <UpdatePanel />
      </div>
    </AnchoredPopover>
  </>;
}
