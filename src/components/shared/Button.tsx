import { Button as SharedButton, type ButtonProps } from "@codeterm/chat-ui";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { BusyState } from "./BusyState";

export type { ButtonProps, ButtonSize, ButtonVariant } from "@codeterm/chat-ui";

export function Button(props: ButtonProps) {
  const touchMode = useTouchOptimizedUi();
  return <SharedButton {...props} touchMode={touchMode} busyIndicator={<BusyState inline hideLabel phase="work" aria-hidden="true" />} />;
}
