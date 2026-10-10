import React, { useState } from "react";
import { Circle, CircleCheck, CircleDashed, ListChecks } from "lucide-react";
import { ChatBlockFrame, TONE_COLOR } from "./ChatBlockFrame";
import { useChatBlockHost, type ChecklistBlockHost } from "./ChatBlockHost";
import { useLiveBinding } from "./liveBinding";
import { BlockNotice } from "./BlockNotice";
import {
  checklistProgress,
  checklistStatusLabel,
  type ChecklistBlockState,
  type ChecklistInfo,
  type ChecklistItemInfo,
  type ChecklistProgress,
} from "./checklistBlockModel";

const MISSING: ChecklistBlockState = { kind: "missing" };

type RenderBlock = (source: string) => React.ReactNode;

/** `@ds-checklist <id>`: steps that tick from real state, each with one click of help. */
export function ChecklistBlock({ checklistId, source, renderBlock }: { checklistId: string; source: string; renderBlock: RenderBlock }) {
  const host = useChatBlockHost().checklists;
  const state = useLiveBinding(host?.binding, checklistId, MISSING);
  if (!host) return <BlockNotice name="checklist" source={source} label={`Checklist ${checklistId}`} text="Checklists are unavailable in this view." icon={ListChecks} />;
  if (state.kind === "missing") return <BlockNotice name="checklist" source={source} label={`Checklist ${checklistId}`} text={`No checklist named “${checklistId}”.`} icon={ListChecks} missing />;
  return <ChecklistCard checklist={state.checklist} host={host} source={source} renderBlock={renderBlock} />;
}

function ChecklistCard({ checklist, host, source, renderBlock }: { checklist: ChecklistInfo; host: ChecklistBlockHost; source: string; renderBlock: RenderBlock }) {
  const progress = checklistProgress(checklist.items);
  const complete = progress.total > 0 && progress.done === progress.total;
  return (
    <ChatBlockFrame
      name="checklist"
      source={source}
      title={checklist.title}
      icon={<ProgressRing progress={progress} />}
      accent={complete ? TONE_COLOR.ok : undefined}
      status={{ label: checklistStatusLabel(progress), tone: complete ? "ok" : "accent" }}
      summary={complete ? "Every step is done." : progress.unknown > 0 ? "Checking your setup…" : "Steps tick themselves as you finish them."}
      body={(
        <ol className="m-0 grid list-none gap-0.5 p-0" aria-label={`${checklist.title} steps`}>
          {checklist.items.map((item) => (
            <ChecklistRow key={item.id} item={item} onOpen={() => host.open(checklist.id, item.id)} renderBlock={renderBlock} />
          ))}
        </ol>
      )}
      askText={`About the ${checklist.title} checklist (\`${source}\`): `}
    />
  );
}

function ChecklistRow({ item, onOpen, renderBlock }: { item: ChecklistItemInfo; onOpen: () => void; renderBlock: RenderBlock }) {
  const [showBlock, setShowBlock] = useState(false);
  const Glyph = item.done === true ? CircleCheck : item.done === null ? CircleDashed : Circle;
  const stateText = item.done === true ? "Done" : item.done === null ? "Checking" : "To do";
  const action = item.action;
  return (
    <li data-checklist-item={item.id} data-done={item.done === null ? "unknown" : String(item.done)} className="min-w-0 rounded-lg px-1 py-1.5">
      <div className="flex min-w-0 items-start gap-2.5">
        <Glyph
          size={16}
          aria-hidden="true"
          className="mt-px shrink-0"
          style={{ color: item.done === true ? TONE_COLOR.ok : "var(--ct-muted-fg)" }}
        />
        <div className="min-w-0 flex-1">
          <p className="m-0 text-[13px] leading-5" style={{ color: item.done === true ? "var(--ct-muted-fg)" : "var(--ct-fg)" }}>
            <span className="sr-only">{stateText}: </span>
            {item.label}
          </p>
          {item.done !== true && <p className="m-0 text-xs leading-relaxed" style={{ color: "var(--ct-muted-fg)" }}>{item.hint}</p>}
        </div>
        {item.done !== true && (
          <button
            type="button"
            data-checklist-action={item.id}
            aria-expanded={action.kind === "block" ? showBlock : undefined}
            onClick={() => (action.kind === "block" ? setShowBlock((open) => !open) : onOpen())}
            className="ct-bar-pill shrink-0"
            style={{ height: 24 }}
          >
            {action.label}
          </button>
        )}
      </div>
      {action.kind === "block" && showBlock && item.done !== true && (
        <div data-testid="checklist-nested-block" className="ml-[26px] min-w-0">{renderBlock(action.source)}</div>
      )}
    </li>
  );
}

function ProgressRing({ progress }: { progress: ChecklistProgress }) {
  const radius = 7.5;
  const circumference = 2 * Math.PI * radius;
  const share = progress.total ? progress.done / progress.total : 0;
  return (
    <svg width="16" height="16" viewBox="0 0 20 20" aria-hidden="true" data-testid="checklist-ring">
      <circle cx="10" cy="10" r={radius} fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="2.5" />
      <circle
        cx="10"
        cy="10"
        r={radius}
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeDasharray={`${circumference * share} ${circumference}`}
        transform="rotate(-90 10 10)"
        className="transition-[stroke-dasharray] duration-500 motion-reduce:transition-none"
      />
    </svg>
  );
}
