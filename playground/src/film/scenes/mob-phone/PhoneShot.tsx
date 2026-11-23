import { MessageBubble } from "@codeterm/chat-ui";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { ArrowUp, Menu, Plus } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { ComposerCard } from "../../../../../src/components/Terminal/ComposerCard";
import { ComposerTextInput } from "../../../../../src/components/Terminal/ComposerTextInput";
import { range, useStage } from "../../engine";
import { phoneAnswer, phoneAsk, phoneConfirm, phoneReply } from "../../fixtures/mob-phone";

type Beat = "open" | "reply" | "answer" | "sync";

function Bubble({ message, t, at }: { message: NormalizedChatMessage; t: number; at: number }) {
  if (t < at) return null;
  const k = range(t, at, at + 0.65);
  return <div style={{ opacity: k, transform: `translateY(${(1 - k) * 18}px)`, marginBottom: 16 }}>
    <MessageBubble message={message} fontScale={1.2} showTimestamp={false} />
  </div>;
}

function Composer({ draft }: { draft: string }) {
  return <div className="ct-chat-bottom-bar" style={{ padding: "12px 10px", "--ct-capsule-font-size": "16px", "--ct-capsule-line-height": "24px" } as CSSProperties}>
    <ComposerCard actions={<div style={{ padding: 8, color: "var(--ct-accent)" }}><ArrowUp size={20} /></div>}>
      <Plus size={17} style={{ flexShrink: 0, color: "var(--ct-muted-fg)" }} />
      <ComposerTextInput aria-label="Message" value={draft} readOnly placeholder="Message" style={{ minHeight: 32, paddingTop: 5, fontSize: 16 }} />
    </ComposerCard>
  </div>;
}

function Conversation({ t, beat, compact = false }: { t: number; beat: Beat; compact?: boolean }) {
  const typing = beat === "answer" && t < 4.15;
  const draft = typing ? String(phoneAnswer.content).slice(0, Math.round(String(phoneAnswer.content).length * range(t, 0.6, 3.5, k => k))) : "";
  return <div className="ct-chat-shell" style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
    <div style={{ height: 48, display: "flex", alignItems: "center", padding: "0 16px", gap: 12, borderBottom: "1px solid var(--ct-border-subtle)" }}>
      <Menu size={18} /><span style={{ fontSize: 16, fontWeight: 600 }}>Helm</span>
    </div>
    <div role="log" aria-label="Conversation" className="ct-chat-surface" style={{ position: "relative", flex: 1, overflow: "hidden" }}>
      <div style={{ position: "absolute", left: 14, right: 14, bottom: compact ? 8 : 28 }}>
        {!compact && <Bubble message={phoneAsk} t={t} at={-1} />}
        {beat !== "open" && !compact && <Bubble message={phoneReply} t={t} at={beat === "reply" ? 1.55 : -1} />}
        {(beat === "answer" || beat === "sync") && <Bubble message={phoneAnswer} t={t} at={beat === "answer" ? 4.15 : -1} />}
        {beat === "sync" && <Bubble message={phoneConfirm} t={t} at={2.3} />}
      </div>
    </div>
    <Composer draft={draft} />
  </div>;
}

function Frame({ children, width, height, phone = true }: { children: ReactNode; width: number; height: number; phone?: boolean }) {
  return <div style={{ width, height, padding: phone ? 7 : 5, borderRadius: phone ? 30 : 13, background: "var(--ct-surface)", border: "1px solid var(--ct-border-default)", boxShadow: "var(--ct-shadow-xl)" }}>
    <div style={{ width: "100%", height: "100%", overflow: "hidden", borderRadius: phone ? 23 : 9 }}>{children}</div>
  </div>;
}

export function PhoneShot({ t, beat }: { t: number; beat: Beat }) {
  const { width, height, portrait } = useStage();
  const k = range(t, 0, 6);
  const sync = beat === "sync";
  const scale = sync ? 2.12 : 2.16 + k * 0.06;
  const angle = (sync ? -2 + k : -1.2 + k * 1.2) * Math.PI / 180;
  const phoneWidth = scale * ((sync ? 260 : 350) * Math.cos(angle) - (sync ? 350 : 540) * Math.sin(angle));
  return <div style={{ position: "absolute", inset: 0, overflow: "hidden", background: "radial-gradient(ellipse at 45% 33%, var(--ct-surface), var(--ct-bg) 72%)" }}>
    <div style={{ position: "absolute", left: (width - 1080) / 2, top: Math.max(0, (height - 1920) / 2), width: 1080, height: 1920 }}>
      {sync && <div style={{ position: "absolute", left: portrait ? (1080 - 390 * (2.12 + k * 0.04)) / 2 : 72, top: 180 - k * 18, transform: `scale(${2.12 + k * 0.04})`, transformOrigin: "0 0" }}>
        <Frame width={390} height={240} phone={false}><Conversation t={t} beat="sync" compact /></Frame>
        <div style={{ height: 9, margin: "1px -9px", borderRadius: "0 0 12px 12px", background: "var(--ct-border-default)" }} />
      </div>}
      <div style={{ position: "absolute", left: portrait ? (1080 - phoneWidth) / 2 : sync ? 350 - k * 20 : 130 - k * 16, top: sync ? 660 - k * 20 : 170 - k * 20, transform: `scale(${sync ? 2.12 : 2.16 + k * 0.06}) rotate(${sync ? -2 + k : -1.2 + k * 1.2}deg)`, transformOrigin: "0 0" }}>
        <Frame width={sync ? 260 : 350} height={sync ? 350 : 540}><Conversation t={t} beat={beat} compact={sync} /></Frame>
      </div>
    </div>
  </div>;
}

export const phoneBeats: Beat[] = ["open", "reply", "answer", "sync"];
export const phoneBeatId = (beat: Beat) => `mob-phone-${beat}`;
