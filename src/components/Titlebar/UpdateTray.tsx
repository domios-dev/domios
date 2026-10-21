import { useCallback, useContext, useEffect, useRef, useState } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { ArrowDownToLine } from "lucide-react";
import { useUpdatePanel } from "../../hooks/useUpdatePanel";
import { MultiplexerActionsContext } from "../../store/multiplexer/context";
import { describeError } from "../../lib/describeError";
import { UpdatePanel } from "../UpdatePanel";
import { BusySpinner } from "../shared/BusyState";

export function UpdateTray() {
  const update = useUpdatePanel();
  const actions = useContext(MultiplexerActionsContext);
  const upsertToast = actions?.upsertToast;
  const [open, setOpen] = useState(false);
  const toastedError = useRef<string | null>(null);
  useEffect(() => {
    if (!update.error) { toastedError.current = null; return; }
    if (!upsertToast || toastedError.current === update.error) return;
    toastedError.current = update.error;
    const description = describeError(update.error, { operation: "updating components" });
    upsertToast({
      id: "component-update-failure",
      title: description.title,
      body: description.cause,
      severity: "Warning",
      level: "action_needed",
      tab_id: null,
      workspace_id: null,
      local_actions: [
        ...(description.raw ? [{ label: "Copy details", run: async () => {
          try { await navigator.clipboard.writeText(description.raw); } catch { return; }
        } }] : []),
        { label: "Retry", run: () => update.retry() },
      ],
    });
  }, [update.error, update.retry, upsertToast]);
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
      onClick={() => setOpen(!open)} title={offers ? "Updates available" : "Update history"}
      className={`group inline-flex h-6 items-center gap-1 rounded-full ${quiet ? "bg-[var(--ct-surface)] text-[var(--ct-muted-fg)]" : "bg-[var(--ct-accent)] text-[var(--ct-on-accent)]"} pl-1.5 pr-2 text-[11px] font-medium hover:brightness-110`}>
      {update.busy ? <BusySpinner size={12} /> : <ArrowDownToLine size={12} />}
      <span role="status" className="max-[430px]:hidden">{update.busy ? "Updating" : quiet ? "Updates" : "Update"}</span>
    </button>
    <AnchoredPopover open={open} anchor={trigger} placement="bottom-end" onClose={close}>
      <div ref={panel} role="dialog" aria-label="Updates" className="w-[400px] max-w-[calc(100vw-24px)] max-h-[min(640px,80vh)] overflow-y-auto">
        <UpdatePanel />
      </div>
    </AnchoredPopover>
  </>;
}
