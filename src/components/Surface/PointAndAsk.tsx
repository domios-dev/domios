import { useId, useRef, useState, type KeyboardEvent } from "react";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";
import { Segmented, type SegmentedOption } from "../shared/Segmented";
import { ASK_INTENTS, askPlaceholder, type AskIntent, type ElementCard } from "./surfaceChromeModel";

export interface PointAndAskProps {
  owner: string;
  card: ElementCard;
  position: { left: number; top: number };
  sending?: boolean;
  onSend(comment: string, intent: AskIntent): void;
  onCancel(): void;
}

const INTENT_OPTIONS: SegmentedOption<AskIntent>[] = ASK_INTENTS.map((intent) => ({ value: intent.value, label: intent.label }));

export function PointAndAsk({ owner, card, position, sending = false, onSend, onCancel }: PointAndAskProps) {
  const comment = useRef<HTMLTextAreaElement | null>(null);
  const [intent, setIntent] = useState<AskIntent>("question");
  const elementId = useId();
  const send = () => { if (!sending) onSend(comment.current?.value ?? "", intent); };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && !event.shiftKey) {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      send();
    }
  };
  return (
    <div
      role="dialog"
      aria-label="Ask about element"
      aria-describedby={elementId}
      className="ct-surface-ask"
      style={{ left: position.left, top: position.top }}
      onKeyDown={onKeyDown}
    >
      <div id={elementId}>
        <div className="ct-surface-ask__element">{card.label}</div>
        {card.meta && <div className="ct-surface-ask__meta">{card.meta}</div>}
      </div>
      <textarea
        autoFocus
        className={`${UI_INPUT.base} ct-surface-ask__comment`}
        aria-label="Your question or request"
        placeholder={askPlaceholder(owner)}
        ref={comment}
      />
      <div className="ct-surface-ask__row">
        <Segmented<AskIntent> aria-label="Intent" value={intent} options={INTENT_OPTIONS} onChange={setIntent} />
        <Button size="sm" className="ct-surface-ask__actions-start" onClick={onCancel}>Cancel</Button>
        <Button size="sm" variant="primary" loading={sending} onClick={send}>Send</Button>
      </div>
    </div>
  );
}
