import "../../fixtures/supporter";
import { Globe, ShieldCheck } from "lucide-react";
import { useEffect, useLayoutEffect, useState, type CSSProperties, type ReactNode } from "react";
import { WebAccessPanel } from "../../../../../src/components/MeshPanel/WebAccessPanel";
import { buildWebUiTarget } from "../../../../../src/components/MeshPanel/webUiTarget";
import { Titlebar } from "../../../../../src/components/Titlebar/Titlebar";
import { WebPeerPicker } from "../../../../../src/components/WebPeerPicker/WebPeerPicker";
import { UpdateProvider } from "../../../../../src/hooks/useUpdate";
import { UpdatePanelProvider } from "../../../../../src/hooks/useUpdatePanel";
import { editFrontendDraft } from "../../../../../src/lib/frontendUpdate/appState";
import { DEFAULT_THEME } from "../../../../../src/lib/themeCss";
import type { Tab, TabStatus } from "../../../../../src/lib/types";
import { clearRelayTarget, setRelayTarget } from "../../../../../src/lib/webApi";
import { ChatBottomBar } from "../../../gallery/ChatBottomBar";
import { agentTab, API, CODEX, ProductProviders } from "../../../gallery/fixtures";
import { PhoneViewport } from "../../../gallery/stageTools";
import { easeInOut, easeOut, lerp, range, Spotlight, useStage, type FocusKey, type Scene } from "../../engine";
import { HISTORY, ORCHESTRATOR } from "../../fixtures/fullui";
import { OrchestratorPane, type ChatRow } from "../../fixtures/fullui-chat";
import { PHONE_MESSAGE, RENDER_FARM_ROWS, WEB_ACTIONS, WEB_CONFIG, WEB_NETWORK, WEB_SERVE } from "../../fixtures/labAnyDevice";
import { FILM_WORKSPACE, MEMBER_STATUS } from "../../fixtures/team";
import { Pulse, Ripple } from "../mesh/meshFixtures";
import { ChatRows, type TimedMessage } from "./labChat";

const DURATION = 9;
const WIN = { w: 1240, h: 820 };
const PHONE = { w: 390, h: 760 };
const PANEL = { right: 20, top: 48, w: 330, h: 470 };

const PANEL_IN = 0.2;
const PANEL_OUT = 2.8;
const PHONE_IN = 0.8;
const LOCK_AT = 2.25;
const CHAT_AT = 2.6;
const TYPE = [3.2, 4.6] as const;
const SEND_AT = 4.8;
const LAND_AT = 5.4;
const TAP_AT = 5.9;
const SWITCH_AT = 6.2;
const BADGE_AT = 7.3;

const WEB_LINK = buildWebUiTarget(WEB_CONFIG, WEB_NETWORK, WEB_SERVE).link;
const DESKTOP_TAB = ORCHESTRATOR;
// Composer drafts are keyed by tab id; a distinct id keeps the phone's typing out of the desktop composer.
const PHONE_TAB: Tab = { ...ORCHESTRATOR, id: `${ORCHESTRATOR.id}-web` };
const RIG_TAB = agentTab("lab-render-farm", { title: "Render farm", workspace_id: FILM_WORKSPACE.id, agent_type: CODEX.id, agent_session_model: "gpt-6-sol", agent_state: { state: "working", status_line: "Rendering shot 14 at 4K" } });
const DESKTOP_ROWS: ChatRow[] = HISTORY.map((card, i) => ({ ...card, at: -1 + i * 0.01 }));
const SENT: ChatRow = { message: PHONE_MESSAGE, at: LAND_AT };

const noop = () => undefined;

function layout(portrait: boolean) {
  return portrait
    ? { win: { x: 106, y: 100, s: 0.7 }, phone: { x: 321.6, y: 610, s: 1.12 }, enter: { x: 0, y: 1300 }, badge: { cx: 540, y: 455, w: 900, size: 30 } }
    : { win: { x: 60, y: 130, s: 1 }, phone: { x: 1395, y: 110, s: 1.15 }, enter: { x: 620, y: 0 }, badge: { cx: 690, y: 975, w: 1000, size: 28 } };
}

/** The phone composer seeds from the draft store on mount; a keyed remount shows this frame's prefix. */
function phoneDraft(t: number) {
  if (t >= SEND_AT || t < TYPE[0]) return "";
  const text = PHONE_MESSAGE.content as string;
  return text.slice(0, Math.round(text.length * range(t, TYPE[0], TYPE[1], (k) => k)));
}

function Messages({ t, rows }: { t: number; rows: TimedMessage[] }) {
  return (
    <div className="ct-chat-surface absolute inset-0 overflow-hidden" role="log" aria-label="Conversation" style={{ background: "var(--ct-bg)" }}>
      <div style={{ position: "absolute", left: 0, right: 0, bottom: 0 }}>
        <ChatRows t={t} rows={rows} />
      </div>
    </div>
  );
}

/** WebPeerPicker reads the relay target once at mount; the target is set only for that mount so no other request is relayed. */
function PickerOn({ peer }: { peer: { id: string; name: string } | null }) {
  if (peer) setRelayTarget(peer.id, peer.name);
  else clearRelayTarget();
  useLayoutEffect(() => clearRelayTarget(), []);
  return <WebPeerPicker compact />;
}

function PhoneChat({ t }: { t: number }) {
  const onRig = t >= SWITCH_AT;
  const tab: Tab = onRig ? RIG_TAB : PHONE_TAB;
  const status: TabStatus = "Working";
  const draft = onRig ? "" : phoneDraft(t);
  editFrontendDraft(`tab-session-chat:${tab.id}`, draft);
  const rows = onRig
    ? RENDER_FARM_ROWS.map((message, i) => ({ message, at: SWITCH_AT + 0.05 + i * 0.18 }))
    : [...HISTORY.slice(-3).map((card, i) => ({ message: card.message, at: CHAT_AT + 0.05 + i * 0.12 })), { message: PHONE_MESSAGE, at: SEND_AT }];
  return (
    <ProductProviders tabs={[tab]} workspaceId={FILM_WORKSPACE.id}>
      <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", background: "var(--ct-bg)" }}>
        <div style={{ height: 40 }} />
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 12px 10px", borderBottom: "1px solid var(--ct-border-subtle)" }}>
          <PickerOn key={onRig ? "rig" : "self"} peer={onRig ? { id: "gpu-rig", name: "gpu-rig" } : null} />
          <span style={{ fontSize: 15, fontWeight: 600, color: "var(--ct-fg)" }}>{tab.title}</span>
        </div>
        <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
          <Messages t={t} rows={rows} />
        </div>
        <PhoneViewport>
          <ChatBottomBar key={`${tab.id}:${draft}`} tab={tab} status={status} workspaceId={FILM_WORKSPACE.id} />
        </PhoneViewport>
      </div>
    </ProductProviders>
  );
}

// Generated once at load so out-of-order seeks never remount into an empty viewfinder.
let viewfinderQr: string | null = null;
const viewfinderQrReady = WEB_ACTIONS.qr(WEB_LINK).then((image) => (viewfinderQr = image));

function Viewfinder({ t }: { t: number }) {
  const [qr, setQr] = useState(viewfinderQr);
  useEffect(() => {
    let live = true;
    if (!qr) void viewfinderQrReady.then((image) => live && setQr(image));
    return () => {
      live = false;
    };
  }, []);
  const scan = range(t, PHONE_IN + 0.5, LOCK_AT, easeInOut);
  const lock = range(t, LOCK_AT, LOCK_AT + 0.25, easeOut);
  const corner = (style: CSSProperties) => <div style={{ position: "absolute", width: 34, height: 34, borderColor: lock > 0 ? "var(--ct-accent)" : "var(--ct-fg)", borderStyle: "solid", borderWidth: 0, ...style }} />;
  const box = lerp(250, 220, lock);
  return (
    <div style={{ position: "absolute", inset: 0, background: "radial-gradient(circle at 50% 42%, var(--ct-surface) 0%, var(--ct-bg) 75%)" }}>
      {qr && <img src={qr} alt="" style={{ position: "absolute", left: "50%", top: "42%", width: 200, height: 200, transform: `translate(-50%, -50%) perspective(600px) rotateY(${lerp(14, 0, scan)}deg) rotateX(${lerp(-8, 0, scan)}deg)`, borderRadius: 10, opacity: 0.92 }} />}
      <div style={{ position: "absolute", left: "50%", top: "42%", width: box, height: box, transform: "translate(-50%, -50%)" }}>
        {corner({ left: 0, top: 0, borderLeftWidth: 4, borderTopWidth: 4, borderTopLeftRadius: 12 })}
        {corner({ right: 0, top: 0, borderRightWidth: 4, borderTopWidth: 4, borderTopRightRadius: 12 })}
        {corner({ left: 0, bottom: 0, borderLeftWidth: 4, borderBottomWidth: 4, borderBottomLeftRadius: 12 })}
        {corner({ right: 0, bottom: 0, borderRightWidth: 4, borderBottomWidth: 4, borderBottomRightRadius: 12 })}
        {lock === 0 && scan > 0 && <div style={{ position: "absolute", left: 10, right: 10, top: `${8 + 84 * ((scan * 2.4) % 1)}%`, height: 2, background: "var(--ct-accent)", boxShadow: "0 0 14px var(--ct-accent)" }} />}
      </div>
      <div style={{ position: "absolute", left: "50%", top: "66%", transform: `translate(-50%, ${(1 - lock) * 10}px)`, opacity: lock, display: "flex", alignItems: "center", gap: 8, padding: "10px 16px", borderRadius: 999, background: "var(--ct-glass-bg, var(--ct-surface))", border: "1px solid var(--ct-border-default)", color: "var(--ct-fg)", fontSize: 14, whiteSpace: "nowrap" }}>
        <Globe size={15} color="var(--ct-accent)" />
        laptop.tail4f2a9c.ts.net
      </div>
    </div>
  );
}

function PhoneFrame({ children }: { children: ReactNode }) {
  return (
    <div style={{ position: "relative", width: PHONE.w, height: PHONE.h, borderRadius: 54, padding: 10, background: "var(--ct-surface)", border: "1px solid var(--ct-border-default)", boxShadow: "var(--ct-shadow-xl)" }}>
      <div style={{ position: "relative", width: "100%", height: "100%", borderRadius: 44, overflow: "hidden", background: "var(--ct-bg)" }}>
        {children}
        <div style={{ position: "absolute", left: "50%", top: 10, width: 108, height: 30, borderRadius: 16, transform: "translateX(-50%)", background: "#000", zIndex: 5 }} />
        <div style={{ position: "absolute", left: 30, top: 14, fontSize: 14, fontWeight: 600, color: "var(--ct-fg)", zIndex: 5 }}>10:00</div>
      </div>
    </div>
  );
}

function DesktopWindow({ t, rows }: { t: number; rows: ChatRow[] }) {
  const panelIn = range(t, PANEL_IN, PANEL_IN + 0.35, easeOut) * (1 - range(t, PANEL_OUT, PANEL_OUT + 0.3));
  return (
    <div style={{ position: "relative", width: WIN.w, height: WIN.h, borderRadius: 14, overflow: "hidden", border: "1px solid var(--ct-border-default)", boxShadow: "var(--ct-shadow-xl)", background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
      <div className="flex flex-col" style={{ width: WIN.w, height: WIN.h, "--ct-composer-bottom-padding": "0.5rem" } as CSSProperties}>
        <UpdateProvider><UpdatePanelProvider>
          <Titlebar api={API} theme={DEFAULT_THEME} onOpenSettings={noop} onOpenExtensions={noop} />
        </UpdatePanelProvider></UpdateProvider>
        <div className="relative flex-1 min-h-0">
          <OrchestratorPane t={t} tab={DESKTOP_TAB} status={MEMBER_STATUS[DESKTOP_TAB.id] ?? "Working"} rows={rows} draft="" />
        </div>
      </div>
      {/* Mounted for the whole shot: the panel renders its QR asynchronously once per mount. */}
      <div style={{ position: "absolute", right: PANEL.right, top: PANEL.top, width: PANEL.w, opacity: panelIn, visibility: panelIn > 0 ? "visible" : "hidden", transform: `translateY(${(1 - panelIn) * -10}px) scale(${lerp(0.96, 1, panelIn)})`, transformOrigin: "top right", zIndex: 20, boxShadow: "var(--ct-shadow-xl)", borderRadius: 16 }}>
        <WebAccessPanel config={WEB_CONFIG} network={WEB_NETWORK} serve={WEB_SERVE} loading={false} actions={WEB_ACTIONS} onAction={async () => undefined} onSetup={async () => undefined} onClose={noop} embedded qrSize={200} />
      </div>
    </div>
  );
}

function Badge({ t, cx, y, w, size }: { t: number; cx: number; y: number; w: number; size: number }) {
  const k = range(t, BADGE_AT, BADGE_AT + 0.5, easeOut);
  if (k <= 0) return null;
  return (
    <div style={{ position: "absolute", left: cx - w / 2, top: y, width: w, display: "flex", justifyContent: "center", opacity: k, transform: `translateY(${(1 - k) * 12}px)`, zIndex: 60 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 16, padding: "14px 26px", borderRadius: 18, background: "var(--ct-bg)", border: "1px solid var(--ct-border-default)", boxShadow: "var(--ct-shadow-lg)" }}>
        <ShieldCheck size={size + 6} color="var(--ct-green)" style={{ flexShrink: 0 }} />
        <div>
          <div style={{ fontSize: size, fontWeight: 600, color: "var(--ct-fg)", letterSpacing: "-0.01em" }}>Inside your private Tailnet</div>
          <div style={{ marginTop: 4, fontSize: size * 0.75, color: "var(--ct-muted-fg)" }}>Device to device · no cloud relay · no public exposure by default</div>
        </div>
      </div>
    </div>
  );
}

function AnyDevice({ t }: { t: number }) {
  const { portrait } = useStage();
  const L = layout(portrait);
  const enter = range(t, PHONE_IN, PHONE_IN + 0.7, easeOut);
  const phoneX = L.phone.x + (1 - enter) * L.enter.x;
  const phoneY = L.phone.y + (1 - enter) * L.enter.y;
  const sx = (x: number) => phoneX + x * L.phone.s;
  const sy = (y: number) => phoneY + y * L.phone.s;
  const from: [number, number] = [sx(PHONE.w / 2), sy(PHONE.h - 70)];
  const to: [number, number] = [L.win.x + 620 * L.win.s, L.win.y + 690 * L.win.s];
  const picker: [number, number] = [sx(70), sy(70)];
  const rows = [...DESKTOP_ROWS, SENT];

  const panelRect = { x: L.win.x + (WIN.w - PANEL.right - PANEL.w) * L.win.s - 16, y: L.win.y + PANEL.top * L.win.s - 16, w: PANEL.w * L.win.s + 32, h: PANEL.h * L.win.s + 32 };
  const phoneRect = { x: L.phone.x - 12, y: L.phone.y - 12, w: PHONE.w * L.phone.s + 24, h: PHONE.h * L.phone.s + 24 };
  const scanRect = { x: Math.min(panelRect.x, phoneRect.x), y: Math.min(panelRect.y, phoneRect.y), w: 0, h: 0 };
  scanRect.w = Math.max(panelRect.x + panelRect.w, phoneRect.x + phoneRect.w) - scanRect.x;
  scanRect.h = Math.max(panelRect.y + panelRect.h, phoneRect.y + phoneRect.h) - scanRect.y;
  const focus: FocusKey[] = [{ t: PHONE_IN }, { t: PHONE_IN + 0.5, rect: scanRect, dim: 0.5 }, { t: CHAT_AT, rect: scanRect, dim: 0.5 }, { t: CHAT_AT + 0.5 }];

  return (
    <div style={{ position: "absolute", inset: 0, background: "radial-gradient(ellipse at 40% 45%, var(--ct-titlebar-bg, var(--ct-surface)) 0%, var(--ct-bg) 70%)" }}>
      <div style={{ position: "absolute", left: L.win.x, top: L.win.y, transform: `scale(${L.win.s})`, transformOrigin: "0 0" }}>
        <DesktopWindow t={t} rows={rows} />
      </div>
      <div style={{ position: "absolute", left: phoneX, top: phoneY, transform: `scale(${L.phone.s}) rotate(${(1 - enter) * 6}deg)`, transformOrigin: "0 0", opacity: enter, zIndex: 10 }}>
        <PhoneFrame>{t < CHAT_AT ? <Viewfinder t={t} /> : <PhoneChat t={t} />}</PhoneFrame>
      </div>
      <svg style={{ position: "absolute", inset: 0, width: "100%", height: "100%", overflow: "visible", zIndex: 30, pointerEvents: "none" }} aria-hidden="true">
        <Ripple t={t} x={sx(PHONE.w / 2)} y={sy(PHONE.h * 0.42)} at={LOCK_AT} size={140} />
        <Pulse t={t} from={from} to={to} start={SEND_AT} end={LAND_AT} r={9} />
        <Ripple t={t} x={to[0]} y={to[1]} at={LAND_AT} size={90} />
        <Ripple t={t} x={picker[0]} y={picker[1]} at={TAP_AT} size={50} />
      </svg>
      <Spotlight t={t} keys={focus} />
      <Badge t={t} {...L.badge} />
    </div>
  );
}

const anyDevice: Scene = {
  id: "lab-any-device",
  duration: DURATION,
  render: (t) => <AnyDevice t={t} />,
};

export default anyDevice;
