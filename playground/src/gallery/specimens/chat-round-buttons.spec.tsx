import { ChatComposerMock, ChatRoundButton, type ChatRoundButtonKind, type ChatRoundButtonState } from "../../ds/ChatRoundButton";
import { defineSpecimen } from "../types";

function ChatRoundButtonSpecimen({ kind = "voice", state = "rest", context = false, disabled = false }: { kind?: ChatRoundButtonKind; state?: ChatRoundButtonState; context?: boolean; disabled?: boolean }) {
  if (context) return <ChatComposerMock state={state} />;
  return <ChatRoundButton kind={kind} state={state} disabled={disabled} />;
}

export default defineSpecimen({
  id: "chat-round-buttons",
  title: "Chat round buttons",
  category: "Buttons",
  where: "src/components/Terminal/ComposerActionButton.tsx",
  notes: "One square footprint, one icon action. The recording specimen shows the shipped expanded voice bubble; the composer specimen keeps the attach/input/action row intact.",
  states: [
    { id: "rest", label: "Rest · voice", props: { kind: "voice", state: "rest" } },
    { id: "hover", label: "Hover · attach", props: { kind: "attach", state: "hover" } },
    { id: "active", label: "Active · send", props: { kind: "send", state: "active" } },
    { id: "disabled", label: "Disabled · attach", props: { kind: "attach", state: "rest", disabled: true } },
    { id: "recording", label: "Recording · voice", props: { kind: "voice", state: "recording" } },
    { id: "context", label: "Composer context", props: { state: "rest", context: true } },
  ],
  Component: ChatRoundButtonSpecimen,
});
