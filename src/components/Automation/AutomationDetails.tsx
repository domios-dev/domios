import { Fragment, useId, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { ChevronRight, X } from "lucide-react";
import { focusLost } from "./focus";
import type { PresenceState } from "../Canvas/usePresence";
import { AUTOMATION_LABEL } from "./styles";
import { CodeBlock } from "@codeterm/chat-ui";
import { nodeInspector } from "./nodeInspector";
import type { ActionLeaf } from "./projection";
import type { AutomationNode } from "./types";

function Facts({ children }: { children: ReactNode }) {
  return <dl className="grid grid-cols-[minmax(6.5rem,auto)_1fr] gap-x-4 gap-y-1.5 break-words [&>dt]:text-[var(--ct-muted-fg)] [&>dd]:min-w-0 [&>dd]:text-[var(--ct-fg)]">{children}</dl>;
}

function Disclosure({ title, children }: { title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const region = useId();
  return <section className="min-w-0">
    <button type="button" className={`${AUTOMATION_LABEL} -mx-1 inline-flex items-center gap-1 rounded px-1 py-0.5 hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]`} aria-expanded={open} aria-controls={region} onClick={() => setOpen(!open)}>
      <ChevronRight size={12} className={`transition-transform motion-reduce:transition-none ${open ? "rotate-90" : ""}`} aria-hidden="true" />{title}
    </button>
    <div id={region} hidden={!open} className="mt-2">{children}</div>
  </section>;
}

interface FrameProps {
  label: string; title: ReactNode; onClose: () => void; children: ReactNode; focusWhenLost?: boolean;
  panelRef?: RefObject<HTMLElement | null>; presence?: PresenceState;
}

/** The app's right side-panel surface: a non-modal region over the graph with its own scroll, Escape and focus-on-open. */
function DetailsFrame({ label, title, onClose, children, focusWhenLost = false, panelRef, presence }: FrameProps) {
  const own = useRef<HTMLElement>(null);
  const panel = panelRef ?? own;
  useLayoutEffect(() => {
    if (!focusWhenLost || focusLost()) panel.current?.focus();
  }, [label, focusWhenLost, panel]);
  return <aside ref={panel} tabIndex={-1} role="region" aria-label={label} data-state={presence} inert={presence === "closing" || undefined} className="plan-side-panel text-[length:var(--ct-font-size-sm)] text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]" onKeyDown={(event) => {
    if (event.code !== "Escape") return;
    event.stopPropagation();
    onClose();
  }}>
    <header className="plan-side-panel__header items-start">
      <div className="min-w-0 flex-1">{title}</div>
      <button type="button" className="plan-side-panel__action" aria-label="Close details" title="Close details (Esc)" onClick={onClose}><X size={15} aria-hidden="true" /></button>
    </header>
    <div className="plan-side-panel__body px-4 py-4">{children}</div>
  </aside>;
}

export function AutomationDetails({ node, action, nodes, onClose, panelRef, presence }: {
  node: AutomationNode | null; action?: ActionLeaf | null;
  nodes: readonly AutomationNode[]; onClose: () => void;
  panelRef?: RefObject<HTMLElement | null>; presence?: PresenceState;
}) {
  if (!node) return <DetailsFrame label="Details: unavailable participant" title={<h3>Participant unavailable</h3>} onClose={onClose} panelRef={panelRef} presence={presence} focusWhenLost>
    <p role="status">The selected node is no longer available.</p>
  </DetailsFrame>;
  const view = nodeInspector(node, action ?? null, nodes);
  return <DetailsFrame label={`Details: ${view.name}`} title={<h3 className="text-[length:var(--ct-font-size-lg)] font-semibold">{view.name}</h3>} onClose={onClose} panelRef={panelRef} presence={presence}>
    <div className="space-y-5">
      <p className="leading-relaxed">{view.purpose}</p>
      <Facts>{view.properties.map(([name, value]) => <Fragment key={name}><dt>{name}</dt><dd>{value}</dd></Fragment>)}</Facts>
      {view.code && <section aria-label="Node code"><CodeBlock text={view.code.text} language={view.code.language} fullscreenScope="viewport" />{view.code.truncated && <p className="text-xs text-[var(--ct-muted-fg)]">Code preview truncated at the source.</p>}</section>}
      {view.codeUnavailable && <p className="text-xs text-[var(--ct-muted-fg)]">Code is unavailable in this view.</p>}
      <Disclosure title="Details"><Facts><dt>ID</dt><dd className="break-all font-mono text-xs">{action?.handler?.id ?? node.identity.id}</dd></Facts></Disclosure>
    </div>
  </DetailsFrame>;
}
