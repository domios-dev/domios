import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Star } from "lucide-react";
import { siteLinks } from "../../../shared/siteLinks";
import { loadConfig, setUiSettings } from "../../lib/clientSettings";
import { nativeBridge } from "../../lib/nativeBridge";
import { Button } from "../shared/Button";
import { UI_TRAY_BTN } from "../shared/design";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { answerRateUs, isRateUsDue, overlaps, parseRateUs, recordActivity, type RateUsAction, type RateUsState } from "./rateUsModel";

/** The top-right corner the invite hangs over, generously sized, for the covered-input check. */
const INVITE_WIDTH = 340;
const INVITE_HEIGHT = 240;
const EDITABLE = "input, textarea, select, [contenteditable=''], [contenteditable='true']";

/** The invite itself: what it says and its three answers. It never takes focus on its own. */
export function RateUsInvite({ id, titleId, onAnswer }: { id: string; titleId: string; onAnswer: (action: RateUsAction) => void }) {
  return (
    <section id={id} role="region" aria-labelledby={titleId} data-testid="rate-us-invite" className="ct-rate-us">
      <div className="flex items-start gap-3">
        <span aria-hidden="true" className="ct-rate-us__mark">
          <Star size={16} strokeWidth={1.75} fill="currentColor" />
        </span>
        <div className="flex min-w-0 flex-col gap-1">
          <h2 id={titleId} className="m-0 text-[13px] font-semibold text-[var(--ct-fg)]">Rate us on GitHub</h2>
          <p className="m-0 text-xs leading-relaxed text-[var(--ct-muted-fg)]">
            If Domios has earned a place in your day, a star helps other developers find it.
          </p>
        </div>
      </div>
      <div className="mt-3 flex items-center gap-2">
        <Button size="compact" variant="primary" className="gap-1.5 whitespace-nowrap" onClick={() => onAnswer("star")}>
          <Star size={12} strokeWidth={2.25} fill="currentColor" aria-hidden="true" />Star on GitHub
        </Button>
        <Button size="compact" variant="ghost" className="whitespace-nowrap" onClick={() => onAnswer("later")}>Later</Button>
        <button type="button" onClick={() => onAnswer("never")}
          className="ml-auto whitespace-nowrap border-0 bg-transparent p-0 text-[11px] text-[var(--ct-muted-fg)] underline-offset-2 hover:text-[var(--ct-fg)] hover:underline">
          Don’t ask again
        </button>
      </div>
    </section>
  );
}

/** True while something else owns the user's attention: a modal, or a focused input in the corner the invite hangs over. */
function attentionElsewhere(): boolean {
  if (document.querySelector("[aria-modal='true']")) return true;
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !active.matches(EDITABLE)) return false;
  const width = document.documentElement.clientWidth;
  return overlaps(active.getBoundingClientRect(), { left: width - INVITE_WIDTH, right: width, top: 0, bottom: INVITE_HEIGHT });
}

/**
 * The "Rate us on GitHub" dropdown at the top-right of the window chrome. A star
 * appears in the tray only once the invite is due; its popover opens by itself
 * once per showing and never during first run or while a modal is open.
 */
export function RateUsTray({ locked = false, now = Date.now }: { locked?: boolean; now?: () => number }) {
  const [state, setState] = useState<RateUsState | null>(null);
  const [open, setOpen] = useState(true);
  const [blocked, setBlocked] = useState(false);
  const [, setTick] = useState(0);
  const current = useRef<RateUsState | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  current.current = state;
  const popoverId = useId();
  const titleId = useId();

  const persist = useCallback((next: RateUsState) => {
    current.current = next;
    setState(next);
    void setUiSettings({ rate_us: next }).catch(() => undefined);
  }, []);

  useEffect(() => {
    let alive = true;
    loadConfig().then((config) => { if (alive) setState(parseRateUs(config.ui?.rate_us)); }).catch(() => undefined);
    return () => { alive = false; };
  }, []);

  const finished = state === null || state.done;
  useEffect(() => {
    if (finished) return;
    const note = () => {
      const previous = current.current;
      if (!previous) return;
      const next = recordActivity(previous, now());
      if (next !== previous) persist(next);
      setBlocked(attentionElsewhere());
      setTick((tick) => tick + 1);
    };
    note();
    const options = { capture: true, passive: true } as const;
    window.addEventListener("pointerdown", note, options);
    window.addEventListener("keydown", note, options);
    window.addEventListener("focusin", note, options);
    const timer = window.setInterval(note, 2000);
    return () => {
      window.removeEventListener("pointerdown", note, options);
      window.removeEventListener("keydown", note, options);
      window.removeEventListener("focusin", note, options);
      window.clearInterval(timer);
    };
  }, [finished, now, persist]);

  const answer = (action: RateUsAction) => {
    if (!current.current) return;
    if (action === "star") void nativeBridge.openUrl(siteLinks.repository);
    persist(answerRateUs(current.current, action, now()));
  };

  const due = state !== null && isRateUsDue(state, now());
  if (locked || !due) return null;
  return (
    <span className="relative flex items-center">
      <button ref={trigger} type="button" aria-label="Rate Domios on GitHub" title="Rate us on GitHub"
        aria-expanded={open && !blocked} aria-controls={popoverId} onClick={() => setOpen((value) => !value)}
        className={`${UI_TRAY_BTN} text-[var(--ct-yellow)]`}>
        <Star size={14} strokeWidth={1.8} fill="currentColor" aria-hidden="true" />
      </button>
      <AnchoredPopover open={open && !blocked} anchor={trigger} placement="bottom-end" gap={8} width={316} onClose={() => setOpen(false)}>
        <span aria-hidden="true" className="ct-rate-us__caret" />
        <RateUsInvite id={popoverId} titleId={titleId} onAnswer={answer} />
      </AnchoredPopover>
    </span>
  );
}
