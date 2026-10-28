import { Fragment, useEffect, useRef, type RefObject } from "react";
import { MENTION_PICKER_LAYOUT, type ComposerMention } from "../../lib/composerMentions";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { BusyState } from "../shared/BusyState";

export function ComposerMentionPicker({ mentions, loading = false, highlighted, onHighlight, onPick, onClose, triggerRef }: {
  loading?: boolean;
  mentions: ComposerMention[];
  highlighted: number;
  onHighlight: (index: number) => void;
  onPick: (mention: ComposerMention) => void;
  onClose: () => void;
  triggerRef: RefObject<HTMLElement | null>;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: "nearest" });
  }, [highlighted]);
  if (!mentions.length && !loading) return null;
  return <AnchoredPopover open anchor={triggerRef} placement={MENTION_PICKER_LAYOUT.placement} width={MENTION_PICKER_LAYOUT.width} keepSide onClose={onClose} style={{ overflow: "hidden" }}>
    <div className="ct-menu flex flex-col" style={{ maxHeight: "inherit" }}>
      <div className="flex items-center justify-between px-2">
        <span className="hidden min-[761px]:inline">Mention</span><button type="button" aria-label="Close mentions" style={{ minHeight: 44, minWidth: 44 }} onClick={onClose}>×</button>
      </div>
      <div ref={list} role="listbox" aria-label="Mention suggestions" style={{ minHeight: 0, maxHeight: MENTION_PICKER_LAYOUT.rowHeight * MENTION_PICKER_LAYOUT.visibleRows, overflowY: "auto", overscrollBehavior: "contain" }}>
        {loading && <BusyState inline label="Loading suggestions…" phase="work" className="px-2 py-3" />}
        {mentions.map((mention, index) => <Fragment key={mention.id}>
          {(index === 0 || mentions[index - 1].group !== mention.group) && <div className="sticky top-0 px-2 text-xs bg-[var(--ct-bg)]">{mention.group}</div>}
          <button type="button" role="option" aria-selected={index === highlighted} aria-label={mention.label}
            onMouseDown={(event) => event.preventDefault()} onMouseEnter={() => onHighlight(index)} onClick={() => onPick(mention)}
            className="ct-menu-row w-full flex-col items-start" style={{ minHeight: MENTION_PICKER_LAYOUT.rowHeight }}>
            <span className="font-medium text-[var(--ct-accent)]">{mention.label}</span>
            <span className="text-xs text-[var(--ct-muted-fg)] text-left">{mention.description}</span>
          </button>
        </Fragment>)}
      </div>
    </div>
  </AnchoredPopover>;
}
