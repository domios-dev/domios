import { useEffect, useRef, useState } from "react";
import { Power } from "lucide-react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { UI_TRAY_BTN } from "../shared/design";
import { LiquidToggleTrack, useLiquidTravel } from "../shared/LiquidToggle";
import { Segmented } from "../shared/Segmented";

import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useKeepAwake } from "./useKeepAwake";

const PRESETS = [
  { value: "inf", label: "∞", title: "Indefinitely", secs: null },
  { value: "1h", label: "1h", title: "1 hour", secs: 3600 },
  { value: "2h", label: "2h", title: "2 hours", secs: 7200 },
  { value: "4h", label: "4h", title: "4 hours", secs: 14400 },
  { value: "8h", label: "8h", title: "8 hours", secs: 28800 },
  { value: "12h", label: "12h", title: "12 hours", secs: 43200 },
] as const;

type PresetValue = (typeof PRESETS)[number]["value"];

function clockParts(expiresAt: number): { h: number; m: number } | null {
  const diff = expiresAt - Date.now();
  if (diff <= 0) return null;
  const totalMins = Math.ceil(diff / 60000);
  return { h: Math.floor(totalMins / 60), m: totalMins % 60 };
}

export function KeepAwakeTray() {
  const [open, setOpen] = useState(false);
  const { status, displaySleep, error, pending, change } = useKeepAwake(open);
  const [picked, setPicked] = useState<PresetValue>("inf");
  const [tick, setTick] = useState(0);
  const triggerRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!status.active || status.expires_at === null) return;
    const id = setInterval(() => setTick((t) => t + 1), 500);
    return () => clearInterval(id);
  }, [status.active, status.expires_at]);

  const {
    flip: flipDisplay,
    travel: displayTravel,
    finishTravel: finishDisplayTravel,
  } = useLiquidTravel(displaySleep, (display) => { void change({ kind: "display", display }); });


  // The strip follows status where it can: a countdown drifts off its preset, so a
  // session begun elsewhere keeps this window's last pick; the header clock rules time.
  useEffect(() => {
    if (status.active && status.expires_at === null) setPicked("inf");
  }, [status.active, status.expires_at]);

  const onPick = async (value: PresetValue) => {
    const preset = PRESETS.find((option) => option.value === value);
    if (preset && await change({ kind: "start", seconds: preset.secs, display: displaySleep })) setPicked(value);
  };

  const timed = status.active && status.expires_at !== null;
  const time = timed ? clockParts(status.expires_at!) : null;
  const colonOn = tick % 2 === 0;
  const titleText = status.active
    ? status.expires_at === null
      ? "Keep Awake: Indefinitely"
      : time
        ? `Keep Awake: ${time.h}:${String(time.m).padStart(2, "0")}`
        : "Keep Awake: expiring…"
    : "Keep Awake";

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        data-testid="keepawake-tray"
        onClick={() => setOpen((v) => !v)}
        title={titleText}
        aria-label={titleText}
        aria-pressed={status.active}
        className={
          status.active
            ? "ct-liquid-interactive flex items-center justify-center w-7 h-7 min-h-[44px] sm:min-h-0 rounded-md text-[var(--ct-accent)] hover:bg-[var(--ct-hover)]"
            : UI_TRAY_BTN
        }
      >
        <Power size={14} />
      </button>

      <AnchoredPopover open={open} anchor={triggerRef} placement="bottom-end" width={260} onClose={() => setOpen(false)}>
        <div
          className="ct-portal-menu ct-menu w-[260px]"
          aria-busy={pending}
        >
          <div className="ct-menu-head">
            <div className="flex min-w-0 items-baseline gap-1.5 px-2 pt-1.5 pb-1">
              <span className="text-[13px] font-semibold text-[var(--ct-fg)]">Keep Awake</span>
              {status.active && (
                <>
                  <span className="text-[11px] text-[var(--ct-muted)]">—</span>
                  <span className="font-mono text-[13px] font-semibold tabular-nums leading-none text-[var(--ct-accent)]">
                    {status.expires_at === null ? "∞" : time ? (
                      <>
                        {time.h}
                        <span style={{ opacity: colonOn ? 1 : 0.25, transition: "opacity 120ms" }}>:</span>
                        {String(time.m).padStart(2, "0")}
                      </>
                    ) : "0:00"}
                  </span>
                </>
              )}
            </div>
          </div>

          <div className="flex items-center gap-2 px-1 py-1">
            <button
              type="button"
              className="ct-keepawake-off"
              aria-pressed={!status.active}
              title="Off"
              aria-label="Turn keep awake off"
              disabled={pending}
              onClick={() => void change({ kind: "stop" })}
            >
              ✕
            </button>
            <Segmented
              aria-label="Keep awake duration"
              className={`flex-1${status.active ? "" : " ct-segmented-idle"}`}
              height={26}
              value={picked}
              onChange={onPick}
              options={PRESETS.map((p) => ({ value: p.value, label: p.label, title: p.title, disabled: pending }))}
            />
          </div>

          {error != null && <div className="px-2 py-1">
            <InlineErrorNotice role="alert" description={{ title: "Keep Awake could not be updated", cause: error, raw: error, actions: [] }} />
          </div>}

          <div role="separator" className="ct-menu-separator" />

          <button
            type="button"
            role="switch"
            aria-checked={displaySleep}
            disabled={pending}
            onClick={flipDisplay}
            className="ct-menu-row"
          >
            <span className="min-w-0 flex-1">
              <span className="block">Keep display awake</span>
              <span className="ct-menu-row__description">
                {displaySleep ? "Display stays on" : "Display can sleep"}
              </span>
            </span>
            <LiquidToggleTrack
              checked={displaySleep}
              travel={displayTravel}
              onTravelEnd={finishDisplayTravel}
            />
          </button>
        </div>
      </AnchoredPopover>
    </>
  );
}
