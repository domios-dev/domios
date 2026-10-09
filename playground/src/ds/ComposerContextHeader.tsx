import { useState, type ReactNode } from "react";
import { CircleCheck, Clock3, Infinity as InfinityIcon, Map as MapIcon, Plus, Timer, Workflow, X } from "lucide-react";
import "./composer-context.css";

export type ContextKind = "blueprints" | "crons" | "delayed";
export type ContextBubbleTone = "active" | "scheduled" | "waiting" | "stopped" | "stashed";

export interface ContextBubble {
  id: string;
  title: string;
  meta?: string;
  tone: ContextBubbleTone;
  dismissible?: boolean;
}

export interface ContextTitleSpec {
  kind: "plan" | "task" | ContextKind;
  prefix: string;
  primary: string;
  secondary?: string;
  extra?: number;
  openable?: boolean;
}

export interface ComposerContextHeaderProps {
  title?: ContextTitleSpec;
  linked?: Partial<Record<ContextKind, ContextBubble[]>>;
  open?: ContextKind | null;
}

const KIND_ORDER: readonly ContextKind[] = ["blueprints", "crons", "delayed"];
const KIND_ICON = { blueprints: Workflow, crons: Clock3, delayed: Timer } as const;
const KIND_LABEL = { blueprints: "Blueprints", crons: "Crons", delayed: "Delayed messages" } as const;
const dismissLabel = (kind: ContextKind, bubble: ContextBubble) => kind === "delayed" ? "Cancel delayed message" : `${bubble.tone === "active" ? "Stop" : "Delete"} ${bubble.title}`;

function Bubble({ kind, bubble }: { kind: ContextKind; bubble: ContextBubble }) {
  const stashed = bubble.tone === "stashed";
  return <span className="ds-ctx-bubble" data-tone={bubble.tone}>
    <button type="button" className="ds-ctx-bubble__open">
      {kind === "delayed"
        ? <>
          {stashed
            ? <InfinityIcon size={13} aria-label="Stashed, sends only when you schedule it" className="ds-ctx-bubble__stash" />
            : <span className="ds-ctx-bubble__meta">{bubble.meta}</span>}
          <span className="ds-ctx-bubble__title ds-ctx-bubble__title--text">{bubble.title}</span>
        </>
        : <>
          <span className="ds-ctx-bubble__dot" aria-hidden="true" />
          <span className="ds-ctx-bubble__title">{bubble.title}</span>
          {bubble.meta && <span className="ds-ctx-bubble__meta">{bubble.meta}</span>}
        </>}
    </button>
    {(kind === "delayed" || bubble.dismissible) && <button type="button" className="ds-ctx-bubble__dismiss" aria-label={dismissLabel(kind, bubble)} title={kind === "delayed" ? "Cancel" : bubble.tone === "active" ? "Stop" : "Delete"}>
      <X size={11} aria-hidden="true" />
    </button>}
  </span>;
}

function ContextTitle({ kind, prefix, primary, secondary, extra, openable }: ContextTitleSpec) {
  const Icon = kind === "plan" ? MapIcon : kind === "task" ? CircleCheck : KIND_ICON[kind];
  const body = <>
    <Icon size={13} aria-hidden="true" className="ds-ctx__title-icon" />
    <span className="ds-ctx__prefix">{prefix}</span>
    <span className="ds-ctx__primary">{primary}</span>
    {extra ? <span className="ds-ctx__extra">+{extra}</span> : null}
    {secondary ? <span className="ds-ctx__secondary">· {secondary}</span> : null}
  </>;
  return openable
    ? <button type="button" className="ds-ctx__title" data-kind={kind} title={`${prefix} ${primary}`}>{body}</button>
    : <span className="ds-ctx__title" data-kind={kind} title={`${prefix} ${primary}`}>{body}</span>;
}

export function ComposerContextHeader({ title, linked = {}, open: initialOpen = null }: ComposerContextHeaderProps) {
  const [requested, setRequested] = useState<ContextKind | null>(initialOpen);
  const [shown, setShown] = useState<ContextKind | null>(initialOpen);
  const kinds = KIND_ORDER.filter((kind) => (linked[kind]?.length ?? 0) > 0);
  const open = requested && kinds.includes(requested) ? requested : null;
  if (!title && kinds.length === 0) return null;

  const toggle = (kind: ContextKind) => {
    setRequested(open === kind ? null : kind);
    setShown(kind);
  };

  const strip = (kind: ContextKind): ReactNode => {
    const bubbles = [...(linked[kind] ?? [])].sort((a, b) => kind === "delayed"
      ? Number(a.tone === "stashed") - Number(b.tone === "stashed")
      : kind === "crons" ? Number(b.tone === "waiting") - Number(a.tone === "waiting") : 0);
    return <>
      {bubbles.map((bubble) => <Bubble key={bubble.id} kind={kind} bubble={bubble} />)}
      {kind === "delayed" && <button type="button" className="ds-ctx-bubble ds-ctx-bubble--add"><Plus size={11} aria-hidden="true" />Schedule</button>}
    </>;
  };

  return <div className="ds-root ds-ctx" data-open-kind={open ?? undefined}>
    <div className="ds-ctx__head">
      {title && <ContextTitle {...title} />}
      {kinds.length > 0 && <div className="ds-ctx__kinds" role="group" aria-label="Linked to this tab">
        {kinds.map((kind) => {
          const Icon = KIND_ICON[kind];
          const count = linked[kind]?.length ?? 0;
          const alert = linked[kind]?.some((bubble) => bubble.tone === "waiting") ?? false;
          return <button
            key={kind}
            type="button"
            className="ds-ctx-kind"
            data-kind={kind}
            data-open={open === kind || undefined}
            aria-pressed={open === kind}
            aria-label={`${count} ${KIND_LABEL[kind].toLowerCase()}${alert ? ", one needs you" : ""}`}
            title={KIND_LABEL[kind]}
            onClick={() => toggle(kind)}
          >
            <Icon size={13} aria-hidden="true" />
            {count > 1 && <span className="ds-ctx-kind__count">{count}</span>}
            {alert && <span className="ds-ctx-kind__alert" aria-hidden="true" />}
          </button>;
        })}
      </div>}
    </div>
    <div className="ds-ctx__reveal" data-open={open !== null || undefined}>
      <div className="ds-ctx__reveal-inner">
        {shown && <div className="ds-ctx__strip" data-kind={shown} role="group" aria-label={KIND_LABEL[shown]}>{strip(shown)}</div>}
      </div>
    </div>
  </div>;
}
