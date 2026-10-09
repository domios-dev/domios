import { useEffect, useState } from "react";
import { ChatRoundButton, type ChatRoundButtonKind, type ChatRoundButtonState } from "../../ds/ChatRoundButton";
import { ChatBottomBar, ChatComposerMock } from "../../ds/ChatComposer";
import { defineSpecimen } from "../types";

const BLOOM_BEAT_MS = 1600;

function useBloomLoop(enabled: boolean): boolean {
  const [send, setSend] = useState(enabled);
  useEffect(() => {
    if (!enabled) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = window.setInterval(() => setSend((value) => !value), BLOOM_BEAT_MS);
    return () => window.clearInterval(timer);
  }, [enabled]);
  return send;
}

type ChatRoundButtonSpecimenProps = {
  kind?: ChatRoundButtonKind;
  state?: ChatRoundButtonState;
  context?: boolean;
  draft?: string;
  bloom?: boolean;
  disabled?: boolean;
};

function ChatRoundButtonSpecimen({ kind = "voice", state = "rest", context = false, draft, bloom = false, disabled = false }: ChatRoundButtonSpecimenProps) {
  const send = useBloomLoop(bloom);
  if (context) return <div className="gallery-specimen-fill"><ChatBottomBar><ChatComposerMock state={state} draft={draft} /></ChatBottomBar></div>;
  return <ChatRoundButton kind={bloom ? (send ? "send" : "voice") : kind} state={state} disabled={disabled} />;
}

export default defineSpecimen({
  id: "chat-round-buttons",
  title: "Chat round buttons",
  category: "Buttons",
  where: "src/components/Terminal/ComposerActionButton.tsx",
  notes: "Composer controls share --ct-control-height (2.125rem) with 17px icons; the inner schedule button uses --ct-control-height-micro. Typing turns the mic into send with an accent bloom that opens from the centre while the bold rounded arrow settles in. The composer states sit inside the floating bottom capsule.",
  states: [
    { id: "rest", label: "Rest · voice", props: { kind: "voice", state: "rest" } },
    { id: "bloom", label: "Mic → send bloom", props: { bloom: true } },
    { id: "send", label: "Send · bloomed", props: { kind: "send", state: "rest" } },
    { id: "hover", label: "Hover · attach", props: { kind: "attach", state: "hover" } },
    { id: "active", label: "Pressed · send", props: { kind: "send", state: "active" } },
    { id: "schedule", label: "Inner · schedule", props: { kind: "schedule", state: "rest" } },
    { id: "disabled", label: "Disabled · attach", props: { kind: "attach", state: "rest", disabled: true } },
    { id: "recording", label: "Recording · voice", props: { kind: "voice", state: "recording" } },
    { id: "context", label: "Composer · empty", props: { state: "rest", context: true } },
    { id: "context-draft", label: "Composer · draft", props: { state: "rest", context: true, draft: "Ship the gallery update" } },
  ],
  Component: ChatRoundButtonSpecimen,
});
