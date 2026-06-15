import type { InteractiveElement } from "@codeterm/chat-core";
import { InteractiveQuestion, type InteractiveAnswerHandler } from "./InteractiveQuestion";

export function InteractiveElementRenderer({
  element,
  onAnswer,
  answered,
}: {
  element: InteractiveElement;
  onAnswer?: InteractiveAnswerHandler;
  answered?: boolean;
}) {
  switch (element.type) {
    case "question":
      return <InteractiveQuestion question={element} onAnswer={onAnswer} answered={answered} />;
    default:
      // Unknown future kind — render nothing rather than the raw payload.
      return null;
  }
}
