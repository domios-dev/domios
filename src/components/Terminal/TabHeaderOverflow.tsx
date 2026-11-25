import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ChevronDown } from "lucide-react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { TAB_BAR_CONTROL_H } from "./tabBarConstants";

function useHeaderOverflow(isActive: boolean) {
  const [chips] = useState(() => document.createElement("div"));
  const inlineSlot = useRef<HTMLDivElement>(null);
  const panelSlot = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [count, setCount] = useState(0);
  const [expanded, setExpanded] = useState(false);
  const [paneWidth, setPaneWidth] = useState(0);
  const open = expanded && isActive;

  useEffect(() => { if (!isActive) setExpanded(false); }, [isActive]);

  useLayoutEffect(() => {
    const inline = inlineSlot.current;
    const slot = open ? panelSlot.current : inline;
    const actions = inline?.parentElement;
    if (!slot || !inline || !actions) return;
    slot.append(chips);
    if (!isActive) return () => { chips.remove(); };
    if (open) panelSlot.current?.focus({ preventScroll: true });

    const bar = inline.closest(".ct-tab-top-bar");
    const measure = () => {
      const pane = bar?.getBoundingClientRect();
      if (open) {
        const end = trigger.current?.getBoundingClientRect().right;
        if (pane && end !== undefined) {
          setPaneWidth(Math.max(0, Math.min(pane.width - 32, end - pane.left - 8)));
        }
        return;
      }
      const bounds = chips.getBoundingClientRect();
      if (bounds.width === 0) return;
      const items = Array.from(chips.children);
      const boxes = items.map(item => item.getBoundingClientRect());
      const occupied = boxes.filter(box => box.width > 0);
      const gap = parseFloat(getComputedStyle(chips).columnGap) || 0;
      const required = occupied.reduce((width, box) => width + box.width, 0) + gap * Math.max(0, occupied.length - 1);
      const triggerSpace = trigger.current
        ? trigger.current.getBoundingClientRect().width + (parseFloat(getComputedStyle(actions).columnGap) || 0)
        : 0;
      const allFit = required <= bounds.width + triggerSpace + 0.5;
      let clipped = 0;
      items.forEach((item, index) => {
        const box = boxes[index];
        const hidden = !allFit && box.width > 0
          && (box.top - bounds.top >= TAB_BAR_CONTROL_H || box.right > bounds.right + 0.5);
        if (item.hasAttribute("data-tab-chip-clipped") !== hidden) {
          item.toggleAttribute("data-tab-chip-clipped", hidden);
        }
        if (hidden) clipped += 1;
      });
      setCount(clipped);
    };
    const sizes = new ResizeObserver(measure);
    const watch = () => {
      sizes.disconnect();
      if (bar) sizes.observe(bar);
      sizes.observe(inline);
      for (const item of chips.children) sizes.observe(item);
      measure();
    };
    const mutations = new MutationObserver(watch);
    mutations.observe(chips, { childList: true, subtree: true, characterData: true });
    watch();
    return () => {
      sizes.disconnect();
      mutations.disconnect();
      chips.remove();
    };
  }, [chips, isActive, open]);

  return { chips, inlineSlot, panelSlot, trigger, count, open, paneWidth, setExpanded };
}

export function TabHeaderOverflow({ children, isActive }: { children: ReactNode; isActive: boolean }) {
  const { chips, inlineSlot, panelSlot, trigger, count, open, paneWidth, setExpanded } = useHeaderOverflow(isActive);
  const panelId = useId();
  const label = open ? "Hide tab details" : `Show all ${count} hidden details`;

  return <>
    <div ref={inlineSlot} className="ct-tab-top-bar__bubbles flex min-w-0 flex-1" />
    {count > 0 && <>
      <button
        ref={trigger}
        type="button"
        data-bubbles-toggle="true"
        className="ct-bar-pill inline-flex shrink-0 items-center justify-center gap-0.5 px-2 text-xs"
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-controls={open ? panelId : undefined}
        aria-label={label}
        title={label}
        onClick={() => setExpanded(!open)}
      >
        +{count}<ChevronDown size={12} aria-hidden />
      </button>
      <AnchoredPopover
        open={open}
        anchor={trigger}
        placement="bottom-end"
        gap={6}
        onClose={() => setExpanded(false)}
        style={{ maxWidth: paneWidth || undefined }}
      >
        <div
          ref={panelSlot}
          id={panelId}
          role="dialog"
          aria-label="Tab details"
          tabIndex={-1}
          className="ct-menu ct-tab-top-bar__overflow"
          style={{ ["--ct-tab-pill-h" as string]: `${TAB_BAR_CONTROL_H}px` }}
        />
      </AnchoredPopover>
    </>}
    {createPortal(children, chips)}
  </>;
}
