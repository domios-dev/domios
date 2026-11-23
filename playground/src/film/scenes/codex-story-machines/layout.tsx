import type { ReactNode } from "react";
import { useStage } from "../../engine";
import { Frame as FixtureFrame } from "../../fixtures/codex-story-machines";

export { Stack, Chat, CREW, CoolingTask, UpdateReceipt } from "../../fixtures/codex-story-machines";

export function Frame({ title, children }: { title: ReactNode; children: ReactNode }) {
  const { portrait } = useStage();
  if (!portrait) return <FixtureFrame title={title}>{children}</FixtureFrame>;
  return <div style={{ position: "absolute", inset: 0, color: "var(--ct-fg)", background: "var(--ct-bg)" }}>
    <div style={{ position: "absolute", left: 115, top: 100, width: 850, fontSize: 80, fontWeight: 550, letterSpacing: "-0.055em", lineHeight: 1.04, textAlign: "center" }}>{title}</div>
    <div style={{ position: "absolute", left: 55, top: 0, width: 1080, height: 1920 }}>{children}</div>
  </div>;
}
