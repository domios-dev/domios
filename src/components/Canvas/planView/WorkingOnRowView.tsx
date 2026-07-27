import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { AlertTriangle, Bot, Clock, GitBranch, MonitorSmartphone, Radio } from "lucide-react";
import type { WorkingOnChip, WorkingOnChipKind, WorkingOnRowModel } from "./workingOnRow";

const TONE_CLASS: Record<WorkingOnChip["tone"], string> = {
  info: "working-on-chip-info",
  amber: "working-on-chip-amber",
  red: "working-on-chip-red",
  grey: "working-on-chip-grey",
};

function ChipIcon({ kind }: { kind: WorkingOnChipKind }) {
  const size = 10;
  if (kind === "model") return <Bot size={size} />;
  if (kind === "tab") return <MonitorSmartphone size={size} />;
  if (kind === "peer") return <Radio size={size} />;
  if (kind === "branch") return <GitBranch size={size} />;
  if (kind === "age") return <Clock size={size} />;
  if (kind === "warn") return <AlertTriangle size={size} />;
  return null;
}

function WorkingOnChipView({ chip, forMeasure = false }: { chip: WorkingOnChip; forMeasure?: boolean }) {
  return (
    <span
      {...(forMeasure ? {} : { "data-testid": `working-on-chip-${chip.kind}` })}
      data-tone={chip.tone}
      className={`working-on-chip ${TONE_CLASS[chip.tone]}`}
      title={`${chip.fieldLabel}: ${chip.label}`}
    >
      <span className="working-on-chip-icon" aria-hidden>
        <ChipIcon kind={chip.kind} />
      </span>
      <span className="working-on-chip-field">{chip.fieldLabel}</span>
      <span className="working-on-chip-value min-w-0 truncate">{chip.label}</span>
    </span>
  );
}

function useChipOverflow(chipCount: number) {
  const rowRef = useRef<HTMLDivElement | null>(null);
  const measureRef = useRef<HTMLDivElement | null>(null);
  const [visibleCount, setVisibleCount] = useState(chipCount);
  const [open, setOpen] = useState(false);

  const remeasure = useCallback(() => {
    const row = rowRef.current;
    const measure = measureRef.current;
    if (!row || !measure) return;
    const chips = Array.from(measure.children) as HTMLElement[];
    if (!chips.length) {
      setVisibleCount(chipCount);
      return;
    }
    const rowWidth = row.clientWidth;
    const moreBtn = 36;
    let used = 0;
    let fit = 0;
    for (const el of chips) {
      const next = used + el.offsetWidth + (fit > 0 ? 4 : 0);
      const reserve = fit < chips.length - 1 ? moreBtn : 0;
      if (next + reserve > rowWidth) break;
      used = next;
      fit += 1;
    }
    setVisibleCount(Math.max(1, fit || chips.length));
  }, [chipCount]);

  useLayoutEffect(() => {
    remeasure();
  }, [chipCount, remeasure]);

  useEffect(() => {
    const row = rowRef.current;
    if (!row || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => remeasure());
    ro.observe(row);
    return () => ro.disconnect();
  }, [remeasure]);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);

  return { rowRef, measureRef, visibleCount, open, setOpen };
}

export interface WorkingOnRowProps {
  model: WorkingOnRowModel;
  variant?: "board" | "list";
}

export function WorkingOnRow({ model, variant = "board" }: WorkingOnRowProps) {
  const chips = model.chips;
  if (!chips.length) return null;

  const { rowRef, measureRef, visibleCount, open, setOpen } = useChipOverflow(chips.length);
  const overflow = chips.length - visibleCount;
  const visible = overflow > 0 ? chips.slice(0, visibleCount) : chips;
  const hidden = overflow > 0 ? chips.slice(visibleCount) : [];

  return (
    <div data-testid="working-on-row" className={`working-on-row working-on-row-${variant}`}>
      <div ref={measureRef} className="working-on-row-measure" aria-hidden>
        {chips.map((c, i) => (
          <WorkingOnChipView key={`m-${c.kind}-${i}`} chip={c} forMeasure />
        ))}
      </div>
      <div ref={rowRef} className="working-on-row-visible">
        {visible.map((c, i) => (
          <WorkingOnChipView key={`${c.kind}-${i}`} chip={c} />
        ))}
        {overflow > 0 && (
          <span className="working-on-overflow-wrap">
            <button
              type="button"
              data-testid="working-on-overflow-trigger"
              className="working-on-overflow-trigger"
              aria-expanded={open}
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                setOpen((v) => !v);
              }}
            >
              +{overflow}
            </button>
            {open && (
              <div
                data-testid="working-on-overflow-popover"
                className="working-on-overflow-popover"
                role="tooltip"
                onPointerDown={(e) => e.stopPropagation()}
              >
                {hidden.map((c, i) => (
                  <WorkingOnChipView key={`h-${c.kind}-${i}`} chip={c} />
                ))}
              </div>
            )}
          </span>
        )}
      </div>
    </div>
  );
}
