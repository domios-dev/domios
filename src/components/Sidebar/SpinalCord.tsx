import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Activity, Bot, Brain, Check, Loader2, Radar } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { useAskGeneral } from "../../help/useAskGeneral";
import { BusySpinner } from "../shared/BusyState";
import { useSupporterResource } from "../Supporter/useSupporterResource";
import { clampFloatingPosition } from "../shared/floatingPosition";

const ASK_PROMPT = "Tell me about Domios's Spinal Cord (the Supporter Agent): what it does for me day to day (memory recall that answers first, noticing a stuck tab, quiet team heartbeats that wake the Orchestrator only when something changed), why it is worth turning on, and what connecting it takes. Keep it short and convincing, then offer to set it up for me yourself. If I say yes, connect it. Answer in my native language, the one I usually write to you in.";

const POPOVER_WIDTH = 304;

function Vertebrae({ className }: { className?: string }) {
  return (
    <svg className={`ct-spinal-vertebrae ${className ?? ""}`} viewBox="0 0 10 16" aria-hidden="true">
      <rect x="1.5" y="0.5" width="7" height="3.6" rx="1.6" />
      <rect x="1" y="6.2" width="8" height="3.6" rx="1.6" />
      <rect x="1.5" y="11.9" width="7" height="3.6" rx="1.6" />
    </svg>
  );
}

const AGENTS = ["General", "Team", "Workers"] as const;
const SENSES = ["Memory", "Tab health", "Heartbeats"] as const;
const ROWS = [22, 70, 118] as const;
const BONES = [10, 32, 54, 76, 98, 120] as const;

function SpinalGraphic() {
  return (
    <svg className="ct-spinal-graphic" viewBox="0 0 280 152" role="img" aria-label="Signals from your agents travel along the spinal cord to memory, tab health and team heartbeats">
      {ROWS.map((y, index) => {
        const spineY = BONES[index * 2 + 1] + 6;
        const inbound = `M58 ${y} C 96 ${y}, 104 ${spineY}, 131 ${spineY}`;
        const outbound = `M149 ${spineY} C 176 ${spineY}, 184 ${y}, 222 ${y}`;
        return (
          <g key={y}>
            <path className="ct-spinal-graphic__line" d={inbound} />
            <path className="ct-spinal-graphic__pulse" style={{ animationDelay: `${index * 0.35}s` }} d={inbound} />
            <path className="ct-spinal-graphic__line" d={outbound} />
            <path className="ct-spinal-graphic__pulse ct-spinal-graphic__pulse--out" style={{ animationDelay: `${0.8 + index * 0.35}s` }} d={outbound} />
            <circle className="ct-spinal-graphic__node" cx="44" cy={y} r="13" />
            <text className="ct-spinal-graphic__label" x="44" y={y + 25}>{AGENTS[index]}</text>
            <circle className="ct-spinal-graphic__node ct-spinal-graphic__node--sense" cx="236" cy={y} r="13" />
            <text className="ct-spinal-graphic__label" x="236" y={y + 25}>{SENSES[index]}</text>
          </g>
        );
      })}
      <line className="ct-spinal-graphic__cord" x1="140" y1="6" x2="140" y2="138" />
      {BONES.map((y, index) => (
        <rect key={y} className="ct-spinal-graphic__bone" style={{ animationDelay: `${index * 0.14}s` }} x="131" y={y} width="18" height="12" rx="5" />
      ))}
    </svg>
  );
}

/** General-line entry to the Spinal Cord: quiet invitation when off, a selling explainer on click. */
export function SpinalCordButton({ api }: { api: ApiConfig | undefined }) {
  const { data } = useSupporterResource<{ connected?: boolean }>(api, "/api/supporter/status");
  const connected = data?.connected === true;
  const [anchor, setAnchor] = useState<{ top: number; left: number } | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const { state, ask } = useAskGeneral();

  useEffect(() => {
    if (!anchor) return;
    const onPointer = (event: MouseEvent) => {
      const target = event.target as Node;
      if (popoverRef.current?.contains(target) || buttonRef.current?.contains(target)) return;
      setAnchor(null);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setAnchor(null);
      buttonRef.current?.focus();
    };
    document.addEventListener("mousedown", onPointer, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointer, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [anchor]);

  const toggle = () => {
    if (anchor) {
      setAnchor(null);
      return;
    }
    const rect = buttonRef.current?.getBoundingClientRect();
    if (!rect) return;
    const pos = clampFloatingPosition(rect.left, rect.bottom + 8, POPOVER_WIDTH, 420, window.innerWidth, window.innerHeight);
    setAnchor({ top: pos.top, left: pos.left });
  };

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        data-testid="spinal-cord-button"
        data-connected={connected || undefined}
        aria-haspopup="dialog"
        aria-expanded={anchor !== null}
        aria-label={connected ? "Spinal Cord, connected" : "Spinal Cord, not connected yet"}
        className="ct-spinal-btn"
        onClick={toggle}
      >
        <Vertebrae />
        <span className="ct-spinal-btn__label">Spinal Cord</span>
        {!connected && <span className="ct-spinal-btn__invite" aria-hidden="true" />}
      </button>
      {anchor && createPortal(
        <div
          ref={popoverRef}
          role="dialog"
          aria-label="Spinal Cord"
          data-testid="spinal-cord-popover"
          className="ct-portal-menu ct-menu ct-spinal-pop fixed"
          style={{ top: anchor.top, left: anchor.left, width: POPOVER_WIDTH }}
        >
          <div className="ct-spinal-pop__head">
            <span className="ct-spinal-pop__badge"><Vertebrae /></span>
            <span className="min-w-0">
              <span className="ct-spinal-pop__title">Spinal Cord</span>
              <span className="ct-spinal-pop__subtitle">{connected ? "Connected · your agents have reflexes" : "The reflexes between your agents"}</span>
            </span>
          </div>
          <SpinalGraphic />
          <ul className="ct-spinal-pop__points">
            <li><Brain size={14} aria-hidden="true" />Memory recall that answers first</li>
            <li><Radar size={14} aria-hidden="true" />Notices a tab that got stuck</li>
            <li><Activity size={14} aria-hidden="true" />Keeps team heartbeats quiet until something changes</li>
          </ul>
          <p className="ct-spinal-pop__nudge">{connected ? "Curious what it has been doing? Ask your General." : "Want it? Ask your General — he explains it and can connect it all for you."}</p>
          <button type="button" className="ct-spinal-pop__ask" data-ask-state={state} disabled={state === "sending"} onClick={() => void ask(ASK_PROMPT)}>
            {state === "sending" ? <BusySpinner icon={Loader2} size={14} /> : state === "sent" ? <Check size={14} aria-hidden="true" /> : <Bot size={14} aria-hidden="true" />}
            {state === "sent" ? "Sent — the General is on it" : state === "error" ? "The General is not reachable" : "Ask the General"}
          </button>
        </div>,
        document.body,
      )}
    </>
  );
}
