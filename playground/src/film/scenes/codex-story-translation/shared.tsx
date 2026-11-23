import type { CSSProperties, ReactNode } from "react";
import { MessageBubble } from "@codeterm/chat-ui";
import { useStage } from "../../engine";

export const ink = "var(--ct-fg)";
export const muted = "var(--ct-muted-fg)";
export const accent = "var(--ct-accent)";

export const mono = "var(--ct-font-mono)";

export function Frame({ step, children }: { step: string; children: ReactNode }) {
  const { portrait } = useStage();
  return <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: ink }}>
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 92 : 66, width: portrait ? 816 : 1696, display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 26, color: muted }}>
      <span style={{ color: ink, fontWeight: 650, letterSpacing: "-.04em" }}>domios<span style={{ color: accent }}>.</span></span>
      <span style={{ fontFamily: mono, fontSize: 21 }}>DREAD DELUSION / {step}</span>
    </div>
    {children}
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 1424 : 1002, width: portrait ? 816 : 1696, borderTop: "1px solid var(--ct-border)", paddingTop: 17, color: muted, fontSize: portrait ? 23 : 21, display: "flex", justifyContent: "space-between" }}>
      <span>Original Russian translation · <span style={{ color: ink }}>LiquidRing</span></span>
      {!portrait && <span>Port, editing & packaging · rollacode</span>}
    </div>
  </div>;
}

export function Label({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return <div style={{ color: accent, fontFamily: mono, fontSize: 24, letterSpacing: ".08em", textTransform: "uppercase", ...style }}>{children}</div>;
}

export function Report({ name, content, width = 520, scale = 1.5, style }: { name: string; content: string; width?: number; scale?: number; style?: CSSProperties }) {
  return <div style={{ width: width * scale, ...style }}>
    <div className="gallery-transcript" style={{ width, transform: `scale(${scale})`, transformOrigin: "top left", padding: 0 }}>
      <MessageBubble fontScale={2} message={{ id: name, type: "user", content, inboundKind: "system" }} origin={{ kind: "tab", title: name, tabIdShort: "", report: true }} />
    </div>
  </div>;
}

export function Sheet({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return <div style={{ background: "var(--ct-surface)", border: "1px solid var(--ct-border)", borderRadius: 18, ...style }}>{children}</div>;
}
