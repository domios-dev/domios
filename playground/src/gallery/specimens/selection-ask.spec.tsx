import { useEffect, useRef } from "react";
import { SelectionAsk } from "../../../../src/components/shared/SelectionAsk";
import { clampFloatingPosition } from "../../../../src/components/shared/floatingPosition";
import { forcePseudo, waitFor } from "../stageTools";
import { defineSpecimen } from "../types";

const OWNER = { tabId: "ask-lead", name: "Release lead" };
const POPOVER_WIDTH = 320;
const TRIGGER_OFFSET = 12;

type Mode = "trigger" | "hover" | "open";

// The document has one selection, so stages take turns: each waits until the previous one has read its own.
let selecting: Promise<void> = Promise.resolve();

/** Selects the quoted words after mount, then lays the portalled trigger or popover out against the stage instead of the viewport. */
function SelectionStage({ mode }: { mode: Mode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const root = ref.current;
    const stage = root?.closest<HTMLElement>(".gallery-stage-instance");
    const quote = root?.querySelector("[data-gallery-quote]");
    if (!stage || !quote) return;
    let alive = true;
    let anchor: DOMRect | null = null;

    const place = (element: HTMLElement) => {
      if (!anchor || element.dataset.galleryPlaced) return;
      element.dataset.galleryPlaced = "true";
      const box = stage.getBoundingClientRect();
      const popover = element.classList.contains("ct-sel-ask__pop");
      const width = popover ? POPOVER_WIDTH : element.offsetWidth;
      const left = anchor.right - box.left - (popover ? POPOVER_WIDTH / 2 : TRIGGER_OFFSET);
      const pos = clampFloatingPosition(left, anchor.bottom - box.top + 6, width, element.offsetHeight, box.width, box.height);
      element.style.setProperty("position", "absolute", "important");
      element.style.setProperty("top", `${pos.top}px`, "important");
      element.style.setProperty("left", `${pos.left}px`, "important");
    };
    const observer = new MutationObserver(() => stage.querySelectorAll<HTMLElement>(".ct-sel-ask, .ct-sel-ask__pop").forEach(place));
    observer.observe(stage, { childList: true, subtree: true });

    const select = async () => {
      if (!alive) return;
      const range = document.createRange();
      range.selectNodeContents(quote);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
      const rects = range.getClientRects();
      anchor = rects[rects.length - 1] ?? range.getBoundingClientRect();
      document.dispatchEvent(new MouseEvent("mouseup"));
      const trigger = await waitFor(() => stage.querySelector<HTMLElement>(".ct-sel-ask"));
      if (!alive || !trigger) return;
      if (mode === "hover") forcePseudo(trigger, "hover");
      if (mode === "open") trigger.click();
    };
    const timer = window.setTimeout(() => {
      selecting = selecting.then(select, select);
    }, 50);
    return () => {
      alive = false;
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [mode]);

  return <div ref={ref} className="gallery-fill gallery-selection">
    <SelectionAsk className="plan-detail-selectable" subject="plan Calm plan detail" owner={OWNER} onJump={() => undefined}>
      <div className="ct-detail">
        <div className="ct-detail__prose">
          <p>Task and plan detail read by importance: role first, then the agent, then the work. <span data-gallery-quote>Every section is its own calm card with one linked task plus N more.</span> Select any text to ask the orchestrator about it.</p>
        </div>
      </div>
    </SelectionAsk>
  </div>;
}

export default defineSpecimen({
  id: "selection-ask",
  title: "Selection ask",
  category: "Plans",
  where: "src/components/shared/SelectionAsk.tsx",
  stage: "wide",
  notes: "Select text inside a plan or task detail to ask the plan's orchestrator about it. Each stage in turn selects the same sentence programmatically and fires the mouseup SelectionAsk listens for; the portalled dot trigger and glass question popover are then laid out against the stage. Selecting other text here works too.",
  Component: SelectionStage,
  states: [
    { id: "trigger", label: "Selection · dot trigger", props: { mode: "trigger" } },
    { id: "hover", label: "Trigger hover · names the owner", props: { mode: "hover" } },
    { id: "open", label: "Question popover open", props: { mode: "open" } },
  ],
});
