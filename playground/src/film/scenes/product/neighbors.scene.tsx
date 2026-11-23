import { Bot, Check, Lock, Mail, ShieldX, Users } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { BreakoutMirror } from "../../breakout";
import { Camera, clamp, easeOut, lerp, range, track, useStage, type Key, type Scene } from "../../engine";
import { ChatRows, type TimedMessage } from "../lab/labChat";
import ines from "../../assets/neighbors/ines.jpg";
import kenji from "../../assets/neighbors/kenji.jpg";
import maya from "../../assets/neighbors/maya.jpg";
import theo from "../../assets/neighbors/theo.jpg";

// Scene seconds, laid on the narration: "Invite a Neighbor" 3.5, "end-to-end encrypted" 8.4, "Your General Agent decides"
// 11.0, "So when Ines asks" 16.2, "answers" 21.1, "Anything else is refused" 24.3.
const BEAT = {
  reveal: [1.6, 3.4],
  invite: 3.5,
  typed: [3.7, 4.8],
  send: 5.0,
  saved: 5.3,
  inviteFlight: [5.5, 6.4],
  request: 6.5,
  accept: 7.3,
  link: [7.5, 8.6],
  lock: 9.1,
  grantAsk: 11.2,
  grantReply: 12.6,
  scopes: 13.6,
  noScope: 15.0,
  question: [16.4, 17.6],
  answer: 20.6,
  reply: [21.4, 22.6],
  stray: [24.2, 25.1],
  refused: 25.4,
  bounce: [25.5, 26.3],
} as const;

const OWNER = "Sam";
const EMAIL = "ines@northlight.studio";
const MONO = '"JetBrains Mono", ui-monospace, monospace';

interface Neighbor {
  id: string;
  name: string;
  machine: string;
  face: string;
  scope: string | null;
  /** Already a Neighbor when the scene opens; Ines joins through the invite. */
  joined: boolean;
}

const NEIGHBORS: Neighbor[] = [
  { id: "ines", name: "Ines", machine: "studio-mac", face: ines, scope: "progress on the launch video", joined: false },
  { id: "theo", name: "Theo", machine: "theo-desktop", face: theo, scope: "status of the export project", joined: true },
  { id: "maya", name: "Maya", machine: "maya-air", face: maya, scope: "design review dates", joined: true },
  { id: "kenji", name: "Kenji", machine: "kenji-pc", face: kenji, scope: null, joined: true },
];

interface Point { x: number; y: number }
interface Layout {
  window: { x: number; y: number; w: number; h: number; sidebar: number };
  /** Face card centre, and where its own machine sits, far past the frame. */
  faces: Record<string, { at: Point; home: Point; side: "left" | "right" }>;
  keys: Key[];
}

const LANDSCAPE: Layout = {
  window: { x: 360, y: 150, w: 1200, h: 780, sidebar: 270 },
  faces: {
    ines: { at: { x: 1745, y: 300 }, home: { x: 2150, y: 170 }, side: "right" },
    theo: { at: { x: 1745, y: 840 }, home: { x: 2150, y: 980 }, side: "right" },
    maya: { at: { x: 175, y: 300 }, home: { x: -230, y: 170 }, side: "left" },
    kenji: { at: { x: 175, y: 840 }, home: { x: -230, y: 980 }, side: "left" },
  },
  // Close on the window, back out to the Neighbors around it, then in on each beat: the dialog, Ines accepting, the
  // General Agent's grant, every scope at once, Porter answering Ines, and Kenji refused.
  keys: [
    { t: 0, x: 960, y: 540, scale: 1.36 },
    { t: BEAT.reveal[0], x: 960, y: 540, scale: 1.36 },
    { t: BEAT.reveal[1], x: 960, y: 540, scale: 1 },
    { t: BEAT.invite + 0.6, x: 1095, y: 500, scale: 1.3 },
    { t: BEAT.inviteFlight[0] + 0.3, x: 1095, y: 500, scale: 1.3 },
    { t: BEAT.request + 0.4, x: 1420, y: 440, scale: 1.3 },
    { t: BEAT.lock + 0.4, x: 1420, y: 440, scale: 1.3 },
    { t: BEAT.grantAsk - 0.2, x: 1095, y: 470, scale: 1.35 },
    { t: BEAT.scopes - 0.2, x: 1095, y: 470, scale: 1.35 },
    { t: BEAT.scopes + 0.8, x: 960, y: 540, scale: 1.04 },
    { t: BEAT.question[0] - 0.4, x: 960, y: 540, scale: 1.04 },
    { t: BEAT.question[0] + 0.5, x: 1330, y: 470, scale: 1.25 },
    { t: BEAT.stray[0] - 0.6, x: 1330, y: 470, scale: 1.25 },
    { t: BEAT.stray[0] + 0.3, x: 760, y: 590, scale: 1.15 },
    { t: 28, x: 780, y: 590, scale: 1.17 },
  ],
};

// The tall frame keeps the window in the middle and moves the Neighbors above and below it.
const PORTRAIT: Layout = {
  window: { x: 465, y: 150, w: 990, h: 800, sidebar: 250 },
  faces: {
    ines: { at: { x: 1250, y: -250 }, home: { x: 1400, y: -1100 }, side: "right" },
    theo: { at: { x: 1250, y: 1350 }, home: { x: 1400, y: 2200 }, side: "right" },
    maya: { at: { x: 670, y: -250 }, home: { x: 520, y: -1100 }, side: "left" },
    kenji: { at: { x: 670, y: 1350 }, home: { x: 520, y: 2200 }, side: "left" },
  },
  keys: [
    { t: 0, x: 960, y: 550, scale: 1.08 },
    { t: BEAT.reveal[0], x: 960, y: 550, scale: 1.08 },
    { t: BEAT.reveal[1], x: 960, y: 550, scale: 0.95 },
    { t: 28, x: 960, y: 556, scale: 0.97 },
  ],
};

const joinedAt = (n: Neighbor) => (n.joined ? 0.6 : BEAT.link[0]);
const fadeOut = (t: number, a: number, b: number) => 1 - range(t, a, b);

/** Where an envelope is along a path of points, eased over its flight. */
function along(points: Point[], k: number): Point {
  const lengths = points.slice(1).map((p, i) => Math.hypot(p.x - points[i].x, p.y - points[i].y));
  let d = easeOut(k) * lengths.reduce((s, l) => s + l, 0);
  for (let i = 0; i < lengths.length; i++) {
    if (d <= lengths[i] || i === lengths.length - 1) {
      const f = clamp(d / lengths[i]);
      return { x: lerp(points[i].x, points[i + 1].x, f), y: lerp(points[i].y, points[i + 1].y, f) };
    }
    d -= lengths[i];
  }
  return points[points.length - 1];
}

/** The point on the window's near edge that a face's channel meets. */
function dock(layout: Layout, id: string): Point {
  const { window: w, faces } = layout;
  const face = faces[id];
  if (face.at.y < w.y) return { x: face.at.x, y: w.y };
  if (face.at.y > w.y + w.h) return { x: face.at.x, y: w.y + w.h };
  return { x: face.side === "right" ? w.x + w.w : w.x, y: face.at.y };
}

function Envelope({ t, points, flight, refused = false }: { t: number; points: Point[]; flight: readonly [number, number]; refused?: boolean }) {
  if (t < flight[0] || t > flight[1] + 0.15) return null;
  const k = clamp((t - flight[0]) / (flight[1] - flight[0]));
  const p = along(points, k);
  const color = refused ? "var(--ct-red)" : "var(--ct-accent)";
  return (
    <div style={{ position: "absolute", left: p.x - 26, top: p.y - 20, width: 52, height: 40, borderRadius: 10, display: "grid", placeItems: "center", background: `color-mix(in srgb, ${color} 18%, var(--ct-bg))`, border: `1.5px solid ${color}`, boxShadow: `0 0 26px color-mix(in srgb, ${color} 45%, transparent)`, opacity: range(t, flight[0], flight[0] + 0.12) * fadeOut(t, flight[1], flight[1] + 0.15) }}>
      <Mail size={20} color={color} />
      <Lock size={12} color={color} style={{ position: "absolute", right: 5, bottom: 4 }} />
    </div>
  );
}

/** A channel: from a face's own machine past the frame, through the face, to the window, drawn as it connects. */
function Channels({ t, layout }: { t: number; layout: Layout }) {
  return (
    <svg width={1920} height={1080} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }} aria-hidden="true">
      {NEIGHBORS.map((n) => {
        const face = layout.faces[n.id];
        const door = dock(layout, n.id);
        const drawn = n.joined ? range(t, 0.6, 1.8) : range(t, BEAT.link[0], BEAT.link[1]);
        const lit = n.id === "ines" ? Math.max(range(t, BEAT.link[0], BEAT.link[1]) * fadeOut(t, BEAT.lock + 0.4, BEAT.lock + 1.4), range(t, BEAT.question[0], BEAT.question[0] + 0.3) * fadeOut(t, BEAT.reply[1], BEAT.reply[1] + 0.6))
          : n.id === "kenji" ? range(t, BEAT.stray[0], BEAT.stray[0] + 0.3) * fadeOut(t, BEAT.bounce[1], BEAT.bounce[1] + 0.5) : 0;
        const inner = Math.hypot(door.x - face.at.x, door.y - face.at.y);
        const outer = Math.hypot(face.home.x - face.at.x, face.home.y - face.at.y);
        return (
          <g key={n.id}>
            <line x1={face.home.x} y1={face.home.y} x2={face.at.x} y2={face.at.y} stroke="var(--ct-accent)" strokeOpacity={0.35 + 0.4 * lit} strokeWidth={2} strokeDasharray={`${outer * drawn} ${outer}`} />
            <line x1={face.at.x} y1={face.at.y} x2={door.x} y2={door.y} stroke="var(--ct-accent)" strokeOpacity={0.5 + 0.5 * lit} strokeWidth={2 + 1.5 * lit} strokeDasharray={`${inner * drawn} ${inner}`} />
          </g>
        );
      })}
    </svg>
  );
}

/** Each Neighbor's own machine, out past the frame where their channel starts. */
function Homes({ t, layout }: { t: number; layout: Layout }) {
  return (
    <>
      {NEIGHBORS.map((n) => {
        const { home } = layout.faces[n.id];
        return (
          <div key={n.id} style={{ position: "absolute", left: home.x - 110, top: home.y - 34, width: 220, padding: "12px 14px", borderRadius: 12, border: "1px solid var(--ct-border-default)", background: "var(--ct-surface)", opacity: range(t, joinedAt(n), joinedAt(n) + 0.8), display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ display: "flex", gap: 4 }}>{["#f85149", "#d29922", "#3fb950"].map((c) => <span key={c} style={{ width: 9, height: 9, borderRadius: 9, background: c }} />)}</span>
            <span style={{ fontFamily: MONO, fontSize: 16, color: "var(--ct-fg)" }}>{n.machine}</span>
            <Lock size={14} color="var(--ct-accent)" style={{ marginLeft: "auto" }} />
          </div>
        );
      })}
    </>
  );
}

function Chip({ children, color = "var(--ct-accent)", style }: { children: ReactNode; color?: string; style?: CSSProperties }) {
  return <span style={{ display: "inline-flex", alignItems: "center", gap: 6, padding: "4px 10px", borderRadius: 999, fontSize: 14, lineHeight: 1.2, color, border: `1px solid color-mix(in srgb, ${color} 45%, transparent)`, background: `color-mix(in srgb, ${color} 10%, transparent)`, whiteSpace: "nowrap", ...style }}>{children}</span>;
}

function FaceCard({ t, n, layout }: { t: number; n: Neighbor; layout: Layout }) {
  const { at, side } = layout.faces[n.id];
  const shown = n.joined ? range(t, 0.4, 1.2) : range(t, BEAT.request - 0.5, BEAT.request + 0.1);
  const scopeAt = BEAT.scopes + ["ines", "theo", "maya"].indexOf(n.id) * 0.35;
  const scope = n.scope ? range(t, scopeAt, scopeAt + 0.4, easeOut) : range(t, BEAT.noScope, BEAT.noScope + 0.4, easeOut);
  const scopeLit = n.id === "ines" ? range(t, 22.2, 22.5) * fadeOut(t, 23.4, 24) : 0;
  const ring = n.id === "ines" ? Math.max(range(t, BEAT.request, BEAT.request + 0.3) * fadeOut(t, BEAT.accept + 0.2, BEAT.accept + 0.6), range(t, BEAT.question[0] - 0.2, BEAT.question[0]) * fadeOut(t, BEAT.question[0] + 0.6, BEAT.question[0] + 1))
    : n.id === "kenji" ? range(t, BEAT.stray[0] - 0.2, BEAT.stray[0]) * fadeOut(t, BEAT.stray[0] + 0.6, BEAT.stray[0] + 1) : 0;
  const width = 250;
  return (
    <div style={{ position: "absolute", left: at.x - width / 2, top: at.y - 62, width, display: "flex", flexDirection: "column", alignItems: "center", gap: 8, opacity: shown, transform: `scale(${0.9 + 0.1 * shown})` }}>
      <div style={{ position: "relative", width: 104, height: 104 }}>
        <img src={n.face} alt="" width={104} height={104} style={{ borderRadius: 104, border: "2px solid var(--ct-border-default)", objectFit: "cover", display: "block" }} />
        <span style={{ position: "absolute", inset: -7, borderRadius: 120, border: "2.5px solid var(--ct-accent)", opacity: ring, transform: `scale(${1 + 0.06 * ring})` }} />
      </div>
      <div style={{ textAlign: "center", lineHeight: 1.25 }}>
        <div style={{ fontSize: 20, fontWeight: 650, color: "var(--ct-fg)" }}>{n.name}</div>
        <div style={{ fontFamily: MONO, fontSize: 14, color: "var(--ct-muted-fg)" }}>{n.machine}</div>
      </div>
      <div style={{ opacity: scope, transform: `translateY(${(1 - scope) * 8}px)`, minHeight: 28 }}>
        {n.scope
          ? <Chip style={{ boxShadow: `0 0 ${18 * scopeLit}px color-mix(in srgb, var(--ct-accent) 60%, transparent)` }}>{n.scope}</Chip>
          : <Chip color="var(--ct-muted-fg)"><Lock size={12} /> no scope</Chip>}
      </div>
      {n.id === "ines" && <InesSide t={t} side={side} />}
      {n.id === "kenji" && <KenjiSide t={t} />}
    </div>
  );
}

/** What Ines sees on her own machine: the request, her question's receipt, then the answer. */
function InesSide({ t, side }: { t: number; side: "left" | "right" }) {
  const toast = range(t, BEAT.request, BEAT.request + 0.35, easeOut) * fadeOut(t, BEAT.accept + 0.25, BEAT.accept + 0.6);
  const pressed = range(t, BEAT.accept - 0.1, BEAT.accept + 0.05);
  const delivered = range(t, BEAT.question[1], BEAT.question[1] + 0.3) * fadeOut(t, BEAT.stray[0], BEAT.stray[0] + 0.5);
  const answer = range(t, BEAT.reply[1], BEAT.reply[1] + 0.35, easeOut) * fadeOut(t, BEAT.stray[0], BEAT.stray[0] + 0.5);
  const box: CSSProperties = { position: "absolute", top: 230, [side === "right" ? "right" : "left"]: -10, width: 300, padding: "14px 16px", borderRadius: 12, border: "1px solid var(--ct-border-default)", background: "var(--ct-surface)", boxShadow: "0 18px 40px -20px rgb(0 0 0 / 0.6)", textAlign: "left" };
  return (
    <>
      {toast > 0 && (
        <div style={{ ...box, opacity: toast, transform: `translateY(${(1 - toast) * 10}px)` }}>
          <div style={{ fontSize: 16, fontWeight: 650, color: "var(--ct-fg)", display: "flex", alignItems: "center", gap: 8 }}><Users size={16} color="var(--ct-accent)" /> New Neighbor request</div>
          <div style={{ fontSize: 14, color: "var(--ct-muted-fg)", margin: "6px 0 10px", lineHeight: 1.45 }}>{OWNER} would like to connect. Open Neighbors in General to respond.</div>
          <div style={{ display: "flex", gap: 8 }}>
            <span style={{ padding: "5px 12px", borderRadius: 8, fontSize: 14, fontWeight: 600, color: "#fff", background: "var(--ct-accent)", transform: `scale(${1 - 0.06 * pressed * (1 - range(t, BEAT.accept + 0.05, BEAT.accept + 0.2))})` }}>Accept {OWNER}</span>
            <span style={{ padding: "5px 12px", borderRadius: 8, fontSize: 14, color: "var(--ct-muted-fg)", border: "1px solid var(--ct-border-default)" }}>Decline {OWNER}</span>
          </div>
        </div>
      )}
      {delivered > 0 && (
        <div style={{ ...box, opacity: delivered }}>
          <div style={{ fontSize: 14, color: "var(--ct-muted-fg)", marginBottom: 6 }}>Message to Neighbor</div>
          <div style={{ fontSize: 15, color: "var(--ct-fg)", lineHeight: 1.4 }}>Has work on the video started yet?</div>
          <div style={{ marginTop: 8, display: "flex", gap: 8, alignItems: "center" }}><Chip color="var(--ct-green)"><Check size={12} /> Delivered</Chip></div>
          {answer > 0 && (
            <div style={{ marginTop: 10, paddingTop: 10, borderTop: "1px solid var(--ct-border-default)", fontSize: 15, color: "var(--ct-fg)", lineHeight: 1.4, opacity: answer }}>
              Yes. The voice-over is recorded, and the first cut is in review.
            </div>
          )}
        </div>
      )}
    </>
  );
}

/** Kenji's receipt: the question was delivered, and the answer is the refusal. */
function KenjiSide({ t }: { t: number }) {
  const shown = range(t, BEAT.stray[1], BEAT.stray[1] + 0.3);
  const refused = range(t, BEAT.bounce[1], BEAT.bounce[1] + 0.3, easeOut);
  if (shown <= 0) return null;
  return (
    <div style={{ position: "absolute", bottom: 222, left: -10, width: 300, padding: "14px 16px", borderRadius: 12, border: "1px solid var(--ct-border-default)", background: "var(--ct-surface)", boxShadow: "0 18px 40px -20px rgb(0 0 0 / 0.6)", textAlign: "left", opacity: shown }}>
      <div style={{ fontSize: 15, color: "var(--ct-fg)", lineHeight: 1.4 }}>Can you send me the client contract?</div>
      <div style={{ marginTop: 8, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
        <Chip color="var(--ct-green)"><Check size={12} /> Delivered</Chip>
        <span style={{ opacity: refused }}><Chip color="var(--ct-red)"><ShieldX size={12} /> Request refused.</Chip></span>
      </div>
    </div>
  );
}

const GRANT: TimedMessage[] = [
  { at: BEAT.grantAsk, message: { id: "nb-grant-ask", type: "user", content: "Ines can ask how the launch video is going. Nothing else." } as TimedMessage["message"] },
  { at: BEAT.grantReply, message: { id: "nb-grant-reply", type: "assistant", content: "Granted **Ines** one scope: *progress on the launch video*. Theo keeps the export project, Maya the design review dates, Kenji has none. Their questions go to your gateway agent, **Porter**." } as TimedMessage["message"] },
];
const ANSWER: TimedMessage[] = [
  { at: BEAT.answer, message: { id: "nb-answer", type: "assistant", content: "Yes. The voice-over is recorded, and the first cut is in review." } as TimedMessage["message"] },
];

/** A Neighbor's question as it lands in the gateway tab: an untrusted turn carrying the scope the General Agent wrote. */
function NeighborTurn({ t, at, name, scope, text, refused }: { t: number; at: number; name: string; scope: string | null; text: string; refused?: number }) {
  const k = range(t, at, at + 0.35, easeOut);
  if (k <= 0) return null;
  return (
    <div style={{ margin: "0 12px 14px", padding: "12px 14px", borderRadius: 12, border: "1px solid var(--ct-border-default)", background: "var(--ct-surface)", opacity: k, transform: `translateY(${(1 - k) * 14}px)` }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 14 }}>
        <Lock size={14} color="var(--ct-accent)" />
        <span style={{ fontWeight: 650, color: "var(--ct-accent)" }}>Neighbor {name}</span>
        <span style={{ color: "var(--ct-muted-fg)" }}>· encrypted</span>
        <span style={{ marginLeft: "auto" }}>{scope ? <Chip>{scope}</Chip> : <Chip color="var(--ct-muted-fg)"><Lock size={12} /> no scope</Chip>}</span>
      </div>
      <div style={{ marginTop: 8, fontSize: 17, color: "var(--ct-fg)" }}>{text}</div>
      {refused !== undefined && refused > 0 && (
        <div style={{ marginTop: 10, display: "flex", alignItems: "center", gap: 8, fontSize: 15, color: "var(--ct-red)", opacity: refused }}>
          <ShieldX size={16} /> Request refused. <span style={{ color: "var(--ct-muted-fg)" }}>Nothing local was opened or shared.</span>
        </div>
      )}
    </div>
  );
}

function Sidebar({ t, active }: { t: number; active: "channel" | "general" | "porter" }) {
  const row = (label: string, sub: string | null, on: boolean, icon: ReactNode) => (
    <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 12px", borderRadius: 10, background: on ? "color-mix(in srgb, var(--ct-accent) 14%, transparent)" : "transparent" }}>
      {icon}
      <div style={{ lineHeight: 1.2 }}>
        <div style={{ fontSize: 16, color: "var(--ct-fg)", fontWeight: on ? 600 : 450 }}>{label}</div>
        {sub && <div style={{ fontSize: 13, color: "var(--ct-accent)" }}>{sub}</div>}
      </div>
    </div>
  );
  const addPressed = range(t, BEAT.invite - 0.25, BEAT.invite - 0.05) * fadeOut(t, BEAT.invite, BEAT.invite + 0.2);
  return (
    <div style={{ padding: "16px 12px", display: "flex", flexDirection: "column", gap: 4, height: "100%", borderRight: "1px solid var(--ct-border-default)" }}>
      <div style={{ fontSize: 12, letterSpacing: 1.2, color: "var(--ct-muted-fg)", padding: "0 12px 6px" }}>GENERAL</div>
      {row("General Agent", null, active === "general", <Bot size={18} color="var(--ct-accent)" />)}
      {row("Porter", "Gateway agent", active === "porter", <Bot size={18} color="var(--ct-muted-fg)" />)}
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "14px 12px 6px", fontSize: 16, fontWeight: 600, color: "var(--ct-fg)" }}><Users size={18} color="var(--ct-accent)" /> Neighbors</div>
      <div style={{ margin: "2px 12px 10px", padding: "7px 12px", borderRadius: 8, fontSize: 14, fontWeight: 600, textAlign: "center", color: "var(--ct-accent)", border: "1px solid color-mix(in srgb, var(--ct-accent) 50%, transparent)", background: `color-mix(in srgb, var(--ct-accent) ${8 + 22 * addPressed}%, transparent)` }}>Add Neighbor</div>
      <div style={{ fontSize: 13, color: "var(--ct-muted-fg)", padding: "4px 12px" }}>Your Neighbors</div>
      {NEIGHBORS.map((n) => {
        const k = range(t, joinedAt(n) + 0.2, joinedAt(n) + 0.7, easeOut);
        return (
          <div key={n.id} style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 12px", opacity: k, transform: `translateX(${(1 - k) * -10}px)`, order: n.joined ? 1 : 2 }}>
            <img src={n.face} alt="" width={28} height={28} style={{ borderRadius: 28, objectFit: "cover" }} />
            <span style={{ fontSize: 15, color: "var(--ct-fg)" }}>{n.name}</span>
          </div>
        );
      })}
    </div>
  );
}

function AddDialog({ t }: { t: number }) {
  const shown = range(t, BEAT.invite, BEAT.invite + 0.3, easeOut) * fadeOut(t, BEAT.inviteFlight[0] + 0.4, BEAT.inviteFlight[0] + 0.8);
  if (shown <= 0) return null;
  const typed = EMAIL.slice(0, Math.round(EMAIL.length * range(t, BEAT.typed[0], BEAT.typed[1], (k) => k)));
  const pressed = range(t, BEAT.send - 0.1, BEAT.send) * fadeOut(t, BEAT.send + 0.05, BEAT.send + 0.2);
  const saved = range(t, BEAT.saved, BEAT.saved + 0.25);
  return (
    <div style={{ position: "absolute", inset: 0, display: "grid", placeItems: "center", background: `rgb(0 0 0 / ${0.35 * shown})` }}>
      <div style={{ width: 560, padding: "26px 28px", borderRadius: 16, border: "1px solid var(--ct-border-default)", background: "var(--ct-surface)", boxShadow: "0 30px 70px -30px rgb(0 0 0 / 0.7)", opacity: shown, transform: `translateY(${(1 - shown) * 14}px)` }}>
        <div style={{ fontSize: 24, fontWeight: 650, color: "var(--ct-fg)", marginBottom: 18 }}>Add Neighbor</div>
        <div style={{ fontSize: 15, color: "var(--ct-muted-fg)", marginBottom: 6 }}>Email address</div>
        <div style={{ padding: "11px 14px", borderRadius: 10, border: "1.5px solid var(--ct-accent)", fontFamily: MONO, fontSize: 18, color: "var(--ct-fg)", minHeight: 24 }}>
          {typed}<span style={{ opacity: t < BEAT.send && Math.floor(t * 3) % 2 === 0 ? 1 : 0 }}>|</span>
        </div>
        <div style={{ fontSize: 14, color: "var(--ct-muted-fg)", margin: "12px 0 18px", lineHeight: 1.5 }}>Enter their Domios account email. Requests and invitations receive the same confirmation to protect account privacy.</div>
        <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
          <span style={{ padding: "9px 16px", borderRadius: 10, fontSize: 16, fontWeight: 600, color: "#fff", background: "var(--ct-accent)", opacity: saved ? 0.55 : 1, transform: `scale(${1 - 0.05 * pressed})` }}>Send request or invitation</span>
          <span style={{ fontSize: 15, color: "var(--ct-green)", opacity: saved, display: "flex", alignItems: "center", gap: 6 }}><Check size={16} /> Request or invitation saved.</span>
        </div>
      </div>
    </div>
  );
}

function ChannelPanel() {
  return (
    <div style={{ padding: "28px 30px", color: "var(--ct-fg)" }}>
      <div style={{ fontSize: 22, fontWeight: 650, display: "flex", alignItems: "center", gap: 10 }}><Lock size={20} color="var(--ct-accent)" /> Encrypted Neighbor channel</div>
      <div style={{ marginTop: 16, display: "flex", gap: 10, alignItems: "center", fontSize: 16 }}>Reception: <Chip color="var(--ct-green)">receiving</Chip></div>
      <div style={{ marginTop: 12, fontSize: 16, color: "var(--ct-muted-fg)" }}>Incoming messages go to your gateway agent.</div>
    </div>
  );
}

function TabHeader({ title, sub }: { title: string; sub: string }) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 10, padding: "16px 22px 14px", borderBottom: "1px solid var(--ct-border-default)" }}>
      <span style={{ fontSize: 19, fontWeight: 650, color: "var(--ct-fg)" }}>{title}</span>
      <span style={{ fontSize: 14, color: "var(--ct-accent)" }}>{sub}</span>
    </div>
  );
}

function AppWindow({ t, layout }: { t: number; layout: Layout }) {
  const { window: w } = layout;
  const general = range(t, BEAT.grantAsk - 0.6, BEAT.grantAsk - 0.2) * fadeOut(t, BEAT.question[0] - 0.3, BEAT.question[0]);
  const porter = range(t, BEAT.question[0] - 0.2, BEAT.question[0] + 0.2);
  const channel = 1 - Math.max(general, porter);
  const active = porter > 0.5 ? "porter" : general > 0.5 ? "general" : "channel";
  const refused = range(t, BEAT.refused, BEAT.refused + 0.3, easeOut);
  const layer: CSSProperties = { position: "absolute", inset: 0, display: "flex", flexDirection: "column" };
  return (
    <div style={{ position: "absolute", left: w.x, top: w.y, width: w.w, height: w.h, borderRadius: 16, border: "1px solid var(--ct-border-default)", background: "var(--ct-bg)", boxShadow: "0 40px 90px -40px rgb(0 0 0 / 0.75)", overflow: "hidden", display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 16px", borderBottom: "1px solid var(--ct-border-default)", fontSize: 14, color: "var(--ct-fg)" }}>
        <span style={{ width: 9, height: 9, borderRadius: 9, background: "var(--ct-green)" }} /> laptop
        <span style={{ marginLeft: "auto", color: "var(--ct-muted-fg)" }}>{OWNER}</span>
      </div>
      <div style={{ flex: 1, display: "grid", gridTemplateColumns: `${w.sidebar}px 1fr`, minHeight: 0 }}>
        <Sidebar t={t} active={active} />
        <div style={{ position: "relative", minWidth: 0 }}>
          <div style={{ ...layer, opacity: channel }}><ChannelPanel /></div>
          <div style={{ ...layer, opacity: general }}>
            <TabHeader title="General Agent" sub="decides what each Neighbor may ask" />
            <div style={{ paddingTop: 18 }}><ChatRows t={t} rows={GRANT} /></div>
          </div>
          <div style={{ ...layer, opacity: porter }}>
            <TabHeader title="Porter" sub="Gateway agent" />
            <div style={{ paddingTop: 18 }}>
              <NeighborTurn t={t} at={BEAT.question[1]} name="Ines" scope="progress on the launch video" text="Has work on the video started yet?" />
              <ChatRows t={t} rows={ANSWER} />
              <NeighborTurn t={t} at={BEAT.stray[1]} name="Kenji" scope={null} text="Can you send me the client contract?" refused={refused} />
            </div>
          </div>
          <AddDialog t={t} />
        </div>
      </div>
    </div>
  );
}

/** spill: the copy past the frame, which carries only the Neighbors, their machines and the channels, never the window. */
function World({ t, layout, spill = false }: { t: number; layout: Layout; spill?: boolean }) {
  const { window: w, faces } = layout;
  const ines = faces.ines;
  const kenji = faces.kenji;
  const main = { x: w.x + w.sidebar + (w.w - w.sidebar) / 2, y: w.y + 240 };
  const add = { x: main.x, y: w.y + w.h / 2 };
  return (
    <div style={{ position: "absolute", left: 0, top: 0, width: 1920, height: 1080 }}>
      <Homes t={t} layout={layout} />
      <Channels t={t} layout={layout} />
      {!spill && <AppWindow t={t} layout={layout} />}
      {NEIGHBORS.map((n) => <FaceCard key={n.id} t={t} n={n} layout={layout} />)}
      <Envelope t={t} points={[add, dock(layout, "ines"), ines.at]} flight={BEAT.inviteFlight} />
      <Envelope t={t} points={[ines.home, ines.at, dock(layout, "ines"), main]} flight={BEAT.question} />
      <Envelope t={t} points={[main, dock(layout, "ines"), ines.at, ines.home]} flight={BEAT.reply} />
      <Envelope t={t} points={[kenji.home, kenji.at, dock(layout, "kenji"), { x: main.x, y: main.y + 260 }]} flight={BEAT.stray} />
      <Envelope t={t} points={[{ x: main.x, y: main.y + 260 }, dock(layout, "kenji"), kenji.at, kenji.home]} flight={BEAT.bounce} refused />
      <LockBadge t={t} layout={layout} />
    </div>
  );
}

/** "End-to-end encrypted" lands on the new channel as it finishes drawing. */
function LockBadge({ t, layout }: { t: number; layout: Layout }) {
  const k = range(t, BEAT.lock - 0.3, BEAT.lock + 0.1, easeOut) * fadeOut(t, BEAT.grantAsk - 0.4, BEAT.grantAsk);
  if (k <= 0) return null;
  const a = layout.faces.ines.at;
  return (
    <div style={{ position: "absolute", left: a.x - 120, top: a.y - 118, width: 240, display: "flex", justifyContent: "center", opacity: k, transform: `scale(${0.85 + 0.15 * k})` }}>
      <Chip style={{ fontSize: 15, background: "var(--ct-bg)" }}><Lock size={14} /> end-to-end encrypted</Chip>
    </div>
  );
}

function Neighbors({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const layout = portrait ? PORTRAIT : LANDSCAPE;
  return (
    <>
      <Camera t={t} width={1920} height={1080} keys={layout.keys}>
        <World t={t} layout={layout} />
      </Camera>
      <BreakoutMirror camera={track(t, layout.keys)} world={{ width: 1920, height: 1080 }}>
        <World t={t} layout={layout} spill />
      </BreakoutMirror>
    </>
  );
}

const neighbors: Scene = {
  id: "neighbors",
  duration: 28,
  render: (t) => <Neighbors t={t} />,
};

export default neighbors;
