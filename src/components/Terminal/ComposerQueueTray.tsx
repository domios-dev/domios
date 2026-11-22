import { useEffect, useId, useRef, useState } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { changeComposerSendQueue } from "./chatSendStatus";
import type { QueuedSendAction, QueuedSendItem } from "./composerQueueModel";

function sendKey({ sendId, correlationId, version }: QueuedSendItem) {
  return { sendId, correlationId, version };
}

export function ComposerQueueTray({ tabId, items, sending, editable }: {
  tabId: string;
  items: readonly QueuedSendItem[];
  sending: boolean;
  editable: boolean;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const dialogId = useId();
  const [show, setShow] = useState(false);
  const [target, setTarget] = useState<QueuedSendItem | null>(null);
  const [text, setText] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const open = show && editable && items.length > 0;
  const close = () => {
    setShow(false);
    setTarget(null);
    setText("");
  };
  useEffect(() => {
    if (open) panel.current?.focus({ preventScroll: true });
  }, [open]);
  useEffect(() => {
    if (open && target) editor.current?.focus({ preventScroll: true });
  }, [open, target]);
  useEffect(() => {
    if (!open) {
      setShow(false);
      setTarget(null);
      setText("");
    }
  }, [open]);

  const submit = async (action: QueuedSendAction) => {
    if (pending) return;
    setPending(true);
    setError(null);
    try {
      const receipt = await changeComposerSendQueue(tabId, action);
      if (receipt.outcome === "unavailable") {
        setError("This message left the editable queue or changed. Check its current status.");
      } else {
        setTarget(null);
        setText("");
        panel.current?.focus({ preventScroll: true });
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The queue change could not be confirmed.");
    } finally {
      setPending(false);
    }
  };
  const queueLabel = `Queued: ${items.length}`;
  const targetCurrent = target && items.some(item => item.sendId === target.sendId && item.version === target.version);
  return <div className="ct-composer-lane mx-auto flex w-full flex-wrap items-center gap-x-2 text-xs text-[var(--ct-muted-fg)] empty:hidden">
    {sending && <span role="status">Sending…</span>}
    {items.length > 0 && (editable ? <button type="button" ref={button} className="ct-bar-pill"
      aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? dialogId : undefined}
      onClick={() => { if (open) close(); else setShow(true); }}>{queueLabel}</button> : <span role="status">{queueLabel}</span>)}
    {error && <span role="alert">{error}</span>}
    <AnchoredPopover open={open} anchor={button} placement="top-start" width={360} onClose={close}>
      <div ref={panel} tabIndex={-1} id={dialogId} role="dialog" aria-label="Queued messages"
        className="ct-menu max-h-[min(55dvh,22rem)] overflow-y-auto p-2">
        <ul className="m-0 grid list-none gap-1 p-0">
          {items.map((item, index) => <li key={item.sendId} className="flex flex-wrap items-center gap-1">
            <span className="min-w-0 flex-1 truncate px-1" title={item.preview}>{item.preview || "Queued message"}</span>
            <button type="button" className="ct-bar-pill" aria-label={`Append to queued message ${index + 1}`}
              disabled={pending} onClick={() => { setTarget(item); setText(""); setError(null); }}>Append</button>
            <button type="button" className="ct-bar-pill" aria-label={`Cancel queued message ${index + 1}`}
              disabled={pending} onClick={() => void submit({ ...sendKey(item), action: "cancel" })}>Cancel</button>
          </li>)}
        </ul>
        {target && <form className="mt-2 grid gap-1" onSubmit={event => {
          event.preventDefault();
          if (targetCurrent && text.trim()) void submit({ ...sendKey(target), action: "append", text });
        }}>
          <label htmlFor={`${dialogId}-addition`}>Text to append</label>
          <textarea ref={editor} id={`${dialogId}-addition`} rows={2} maxLength={2046} value={text}
            onChange={event => setText(event.target.value)} className="w-full rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] p-1" />
          {!targetCurrent && <p role="status">This message is no longer editable at this version.</p>}
          <button type="submit" className="ct-bar-pill justify-self-start" disabled={pending || !targetCurrent || !text.trim()}>Save addition</button>
        </form>}
      </div>
    </AnchoredPopover>
  </div>;
}
