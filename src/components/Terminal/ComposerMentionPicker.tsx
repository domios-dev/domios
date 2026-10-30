import { useEffect, useRef, type CSSProperties, type RefObject } from "react";
import { CheckSquare, File, Layers, Terminal, Workflow, X } from "lucide-react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { MENTION_PICKER_LAYOUT, type ComposerMention } from "../../lib/composerMentions";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { ActivityDots } from "../shared/ActivityDots";
import { AgentIcon } from "../AgentIcon";
import { Chip } from "../shared/Chip";
import { Button } from "../shared/Button";

export function ComposerMentionPicker({ active = true, listboxId = "composer-mentions", optionId = (row) => `${listboxId}-${encodeURIComponent(row.id)}`, query = "", mentions, loading = false, error = false, highlighted, onHighlight, onPick, onClose, triggerRef }: {
  active?: boolean;
  listboxId?: string;
  optionId?: (row: ComposerMention) => string;
  query?: string;
  loading?: boolean;
  error?: boolean;
  mentions: ComposerMention[];
  highlighted: number;
  onHighlight: (index: number) => void;
  onPick: (mention: ComposerMention) => void;
  onClose: () => void;
  triggerRef: RefObject<HTMLElement | null>;
}) {
  const list = useRef<HTMLDivElement>(null);
  const selectedId = mentions[highlighted]?.id;
  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: "nearest" });
  }, [selectedId]);
  const groups = mentions.reduce<{ name: string; start: number; rows: ComposerMention[] }[]>((all, row, index) => {
    if (all[all.length - 1]?.name !== row.group) all.push({ name: row.group, start: index, rows: [] });
    all[all.length - 1].rows.push(row);
    return all;
  }, []);
  if (!groups.length) groups.push({ name: "Suggestions", start: 0, rows: [] });
  return <AnchoredPopover open={active} anchor={triggerRef} placement={MENTION_PICKER_LAYOUT.placement} width={MENTION_PICKER_LAYOUT.width} keepSide onClose={onClose} className="composer-mention-popover" style={{ overflow: "hidden", "--mention-anchor-width": `${triggerRef.current?.getBoundingClientRect().width ?? 480}px` } as CSSProperties}>
    <div className="ct-menu composer-mention-menu">
      <div ref={list} id={listboxId} role="listbox" aria-label="Mention suggestions" aria-busy={loading} className="composer-mention-list" style={{ maxHeight: MENTION_PICKER_LAYOUT.rowHeight * MENTION_PICKER_LAYOUT.visibleRows }}>
        {groups.map((group, groupIndex) => <div key={`${group.name}-${groupIndex}`} role="group" aria-labelledby={`${listboxId}-group-${groupIndex}`}>
          {groupIndex > 0 && <div className="ct-menu-separator" role="presentation" />}
          <div className="ct-menu-caption composer-mention-heading">
            <span id={`${listboxId}-group-${groupIndex}`}>{group.name === "Tabs · Team" ? "Tabs in this Team" : group.name}</span>
            {groupIndex === 0 && <>
              <span className="composer-mention-busy" role={loading ? "status" : undefined} aria-label={loading ? "Updating suggestions" : undefined}>
                {loading && <DeferredLoader state="present"><ActivityDots size="small" tone="muted" /></DeferredLoader>}
              </span>
              <Button variant="ghost" className="composer-mention-close" aria-label="Close mentions" tabIndex={-1} onPointerDown={(event) => event.preventDefault()} onMouseDown={(event) => event.preventDefault()} onClick={onClose}><X size={14} /></Button>
            </>}
          </div>
          {group.rows.map((mention, offset) => {
            const index = group.start + offset;
            const Icon = mention.kind === "task" ? CheckSquare : mention.kind === "plan" ? Layers : mention.kind === "blueprint" ? Workflow : mention.kind === "file" ? File : Terminal;
            return <div key={mention.id} id={optionId(mention)} role="option" aria-selected={index === highlighted} aria-label={mention.label} aria-describedby={`${optionId(mention)}-description`}
              onPointerDown={(event) => event.preventDefault()} onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => onHighlight(index)} onClick={() => onPick(mention)} className="ct-menu-row composer-mention-row" title={mention.description}>
              <span className="ct-menu-row__icon" data-kind={mention.kind} aria-hidden="true">{mention.providerId ? <AgentIcon providerId={mention.providerId} /> : <Icon size={15} />}</span>
              <span className="composer-mention-label" data-kind={mention.kind}>{matchedLabel(mention.label, query)}</span>
              <span id={`${optionId(mention)}-description`} className="composer-mention-meta">{mention.description}</span>
              {mention.status && <Chip variant="muted">{mention.status.replaceAll("_", " ")}</Chip>}
            </div>;
          })}
        </div>)}
        {!mentions.length && <div className="composer-mention-empty" role="status">
          <span>{loading ? "Searching suggestions" : error ? "Suggestions unavailable" : `No matches${query ? ` for “${query}”` : ""}`}</span>
          <span>{loading ? "Keep typing to refine the search." : "Try another name or keep writing."}</span>
        </div>}
      </div>
    </div>
  </AnchoredPopover>;
}

function matchedLabel(label: string, query: string) {
  const start = query ? label.toLowerCase().indexOf(query.toLowerCase()) : -1;
  return start < 0 ? label : <>{label.slice(0, start)}<strong>{label.slice(start, start + query.length)}</strong>{label.slice(start + query.length)}</>;
}
