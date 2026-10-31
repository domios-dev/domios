import { useEffect, useId, useRef } from "react";
import { ArrowUp, X } from "lucide-react";
import { FormDialog } from "../shared/FormDialog";

export function ExpandedComposer({ value, onChange, onClose, onSend, sendDisabled }: {
  value: string;
  onChange: (value: string) => void;
  onClose: () => void;
  onSend: () => void;
  sendDisabled: boolean;
}) {
  const titleId = useId();
  const editor = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { editor.current?.focus(); }, []);

  return <FormDialog titleId={titleId} onCancel={onClose}
    className="ct-expanded-composer"
    onSubmit={(event) => { event.preventDefault(); if (!sendDisabled) onSend(); }}>
    <header className="flex shrink-0 items-center justify-between gap-3">
      <h2 id={titleId} className="text-sm font-medium">Edit message</h2>
      <button type="button" className="ct-expanded-composer__close" aria-label="Close expanded editor" onClick={onClose}>
        <X size={18} aria-hidden />
      </button>
    </header>
    <textarea ref={editor} aria-label="Expanded message" value={value}
      placeholder="Write your message…" spellCheck={false}
      className="ct-expanded-composer__input"
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
          event.stopPropagation();
          return;
        }
        if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
          event.preventDefault();
          if (!sendDisabled) onSend();
        }
      }} />
    <footer className="flex shrink-0 items-center justify-between gap-3">
      <span className="text-xs" style={{ color: "var(--ct-muted-fg)" }}>Your draft stays in sync.</span>
      <button type="submit" disabled={sendDisabled} className="ct-expanded-composer__send">
        Send <ArrowUp size={15} aria-hidden />
      </button>
    </footer>
  </FormDialog>;
}

export function ComposerExpandHandle({ onExpand }: { onExpand: () => void }) {
  return <button type="button" className="ct-composer-expand" aria-label="Expand message editor" onClick={onExpand}>
    <svg width="36" height="8" viewBox="0 0 36 8" aria-hidden="true"><path d="M1 6 L18 2 L35 6" /></svg>
    <span>Show</span>
  </button>;
}
