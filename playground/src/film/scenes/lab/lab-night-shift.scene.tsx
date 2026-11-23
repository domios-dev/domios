import { Moon, Sun } from "lucide-react";
import { clamp, easeOut, lerp, range, useStage, type Scene } from "../../engine";
import { CHAT_CARDS, CHAT_REPLY } from "../../fixtures/chat";
import { ChatRows, type TimedMessage } from "./labChat";

const DURATION = 8;
const DUSK = [0.6, 1.6] as const;
/** In portrait the reports panel fills the lower part of the frame; the night tint stops above it. */
const PH_NIGHT_CLEAR = 1920 - 700;
const DAWN = [6.2, 7.0] as const;
/** Film clock runs 22:00 to 07:00 across the night. */
const CLOCK = { from: 22 * 60, to: 31 * 60, start: 0.4, end: 6.8 };
const FIRES = CHAT_CARDS.slice(0, 5).map((_, i) => 2.0 + i * 0.85);
const ROWS: TimedMessage[] = [
  ...CHAT_CARDS.slice(0, 5).map((card, i) => ({ ...card, at: FIRES[i] })),
  { ...CHAT_REPLY, at: 7.0 },
];

function minutesAt(t: number) {
  return lerp(CLOCK.from, CLOCK.to, range(t, CLOCK.start, CLOCK.end, (k) => k));
}

function clockText(minutes: number) {
  const m = Math.floor(minutes) % (24 * 60);
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}

/** Moon sets and sun rises along one arc; night tint follows the same curve. */
function Sky({ t, w, h }: { t: number; w: number; h: number }) {
  const night = range(t, DUSK[0], DUSK[1]) * (1 - range(t, DAWN[0], DAWN[1]));
  const moonK = range(t, DUSK[0], DAWN[1], (k) => k);
  const sunK = range(t, DAWN[0] - 0.3, DURATION, easeOut);
  const arc = (k: number): [number, number] => [lerp(w * 0.08, w * 0.92, k), h * 0.92 - Math.sin(Math.PI * k) * h * 0.72];
  const [mx, my] = arc(lerp(0.12, 0.88, moonK));
  const [sx, sy] = arc(lerp(0.0, 0.3, sunK));
  return (
    <svg data-film-anchor="sky" width={w} height={h} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }} aria-hidden="true">
      <path d={`M ${w * 0.04} ${h * 0.92} Q ${w / 2} ${h * -0.52} ${w * 0.96} ${h * 0.92}`} fill="none" stroke="var(--ct-border-default)" strokeWidth={2} strokeDasharray="2 10" strokeLinecap="round" />
      <line x1={0} y1={h * 0.92} x2={w} y2={h * 0.92} stroke="var(--ct-border-default)" strokeWidth={2} />
      {FIRES.map((at, i) => {
        const k = range(t, at - 0.15, at + 0.6, easeOut);
        if (k <= 0 || k >= 1) return null;
        return <circle key={i} cx={mx} cy={my} r={26 + 70 * k} fill="none" stroke="var(--ct-accent)" strokeWidth={2.5} opacity={1 - k} />;
      })}
      <g data-film-anchor="moon" opacity={night} transform={`translate(${mx - 24} ${my - 24})`}>
        <Moon size={48} color="var(--ct-fg)" />
      </g>
      <g data-film-anchor="sun" opacity={sunK} transform={`translate(${sx - 28} ${sy - 28})`}>
        <Sun size={56} color="var(--ct-yellow)" />
      </g>
    </svg>
  );
}

function NightShift({ t }: { t: number }) {
  const { portrait } = useStage();
  const night = range(t, DUSK[0], DUSK[1]) * (1 - range(t, DAWN[0], DAWN[1]));
  const fired = FIRES.filter((at) => t >= at).length;
  const minutes = minutesAt(t);
  const morning = range(t, DAWN[1] - 0.2, DAWN[1] + 0.4, easeOut);
  const tick = Math.max(0, ...FIRES.filter((at) => t >= at).map((at) => 1 - clamp((t - at) / 0.5)));
  const jev = range(t, 0.9, 1.5, easeOut);
  // The reports panel arrives with the first report instead of standing empty.
  const panel = range(t, FIRES[0] - 0.5, FIRES[0] + 0.1, easeOut);
  const L = portrait
    ? { sky: { x: 90, y: 120, w: 900, h: 300 }, clock: { x: 95, y: 470, size: 150 }, chat: { x: 95, y: 720, w: 890, h: 740, s: 1.35 } }
    : { sky: { x: 110, y: 150, w: 620, h: 260 }, clock: { x: 110, y: 470, size: 168 }, chat: { x: 820, y: 90, w: 1000, h: 900, s: 1.3 } };
  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
      <div style={{ position: "absolute", inset: 0, background: "radial-gradient(ellipse at 25% 15%, color-mix(in srgb, var(--ct-yellow) 14%, transparent) 0%, transparent 55%)", opacity: 1 - night }} />
      <div style={{ position: "absolute", left: L.sky.x, top: L.sky.y }}>
        <Sky t={t} w={L.sky.w} h={L.sky.h} />
      </div>
      <div style={{ position: "absolute", left: L.clock.x, top: L.clock.y }}>
        <div style={{ fontFamily: "var(--ct-font-mono)", fontSize: L.clock.size, fontWeight: 600, letterSpacing: "-0.04em", color: "var(--ct-fg)", lineHeight: 1, fontVariantNumeric: "tabular-nums", transform: `scale(${1 + 0.03 * tick})`, transformOrigin: "0 50%" }}>
          {clockText(minutes)}
        </div>
        <div style={{ marginTop: 22, fontSize: portrait ? 34 : 30, color: "var(--ct-muted-fg)", display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ width: 12, height: 12, borderRadius: 6, background: fired ? "var(--ct-accent)" : "var(--ct-border-default)" }} />
          {morning > 0.5 ? `${fired} reports while you slept` : fired ? `${fired} of ${FIRES.length} crons fired` : "Night shift"}
        </div>
        <div data-film-anchor="jev" style={{ marginTop: 26, display: "inline-flex", alignItems: "center", gap: 12, padding: portrait ? "12px 22px" : "10px 18px", borderRadius: 999, border: "1px solid color-mix(in srgb, var(--ct-accent) 45%, var(--ct-border-default))", background: "color-mix(in srgb, var(--ct-accent) 10%, var(--ct-bg))", fontSize: portrait ? 30 : 24, color: "var(--ct-fg)", opacity: jev, transform: `translateY(${(1 - jev) * 10}px)` }}>
          <span style={{ width: 10, height: 10, borderRadius: 5, background: "var(--ct-accent)", boxShadow: `0 0 ${6 + 14 * tick}px var(--ct-accent)` }} />
          <span style={{ fontWeight: 600 }}>Jev</span>
          <span style={{ color: "var(--ct-muted-fg)" }}>keeping the night moving</span>
        </div>
      </div>
      <div style={{ position: "absolute", left: L.chat.x, top: L.chat.y, width: L.chat.w, height: L.chat.h, borderRadius: 16, border: "1px solid var(--ct-border-default)", background: "var(--ct-bg)", boxShadow: "var(--ct-shadow-lg)", overflow: "hidden", opacity: panel, transform: `translateY(${(1 - panel) * 16}px)` }}>
        <div style={{ position: "absolute", left: 0, bottom: 0, width: L.chat.w / L.chat.s, transform: `scale(${L.chat.s})`, transformOrigin: "0 100%" }}>
          <ChatRows t={t} rows={ROWS} padding="0.75rem" />
        </div>
      </div>
      {/* Night darkens the sky and clock side only; the reports stay lit. */}
      <div style={{ position: "absolute", inset: portrait ? `0 0 ${PH_NIGHT_CLEAR}px 0` : `0 ${1920 - L.chat.x + 30}px 0 0`, pointerEvents: "none", background: "color-mix(in srgb, var(--ct-bg) 100%, transparent)", opacity: 0.42 * night, mixBlendMode: "multiply" }} />

      <div style={{ position: "absolute", inset: 0, pointerEvents: "none", background: "color-mix(in srgb, var(--ct-yellow) 8%, transparent)", opacity: morning * (1 - range(t, 7.4, DURATION)) }} />
    </div>
  );
}

const nightShift: Scene = {
  id: "lab-night-shift",
  duration: DURATION,
  render: (t) => <NightShift t={t} />,
};

export default nightShift;
