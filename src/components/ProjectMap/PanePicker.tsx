import { useEffect, useRef, useState } from "react";
import type { DispatchablePane } from "./hooks/useDispatchablePanes";

interface Props {
  panes: DispatchablePane[];
  selectedPaneId: string | null;
  onChange: (paneId: string) => void;
}

export function PanePicker({ panes, selectedPaneId, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (!ref.current) return;
      if (!ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, []);

  const current = panes.find((p) => p.pane_id === selectedPaneId);
  const label = current ? formatPane(current) : panes.length === 0 ? "No targets" : "Pick target…";

  if (panes.length === 1 && current) {
    return (
      <div className="px-2 py-1 rounded-sm border border-[var(--ct-border)] text-xs text-[var(--ct-fg)] bg-[var(--ct-bg)]">
        {label}
      </div>
    );
  }

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        className="px-2 py-1 rounded-sm border border-[var(--ct-border)] text-xs text-[var(--ct-fg)] bg-[var(--ct-bg)] hover:bg-[var(--ct-hover)]"
        onClick={() => setOpen((o) => !o)}
        disabled={panes.length === 0}
      >
        {label}
        <span className="ml-2 opacity-60">▾</span>
      </button>
      {open && panes.length > 0 && (
        <div className="absolute left-0 top-full mt-1 z-20 min-w-[220px] max-h-64 overflow-auto rounded-sm border border-[var(--ct-border)] bg-[var(--ct-bg)] shadow">
          {panes.map((p) => (
            <button
              key={p.pane_id}
              type="button"
              className={`block w-full text-left px-2 py-1 text-xs hover:bg-[var(--ct-hover)] ${
                p.pane_id === selectedPaneId ? "text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)]"
              }`}
              onClick={() => {
                onChange(p.pane_id);
                setOpen(false);
              }}
            >
              {formatPane(p)}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function formatPane(p: DispatchablePane): string {
  const role = p.role.toLowerCase();
  const titleMax = 26;
  const title = p.title.length > titleMax ? `${p.title.slice(0, titleMax - 1)}…` : p.title;
  return `${role} · ${title} · ${p.pane_id.slice(0, 7)}`;
}
