import { InlineErrorNotice } from "../shared/ErrorNotice";
import { describeError } from "../../lib/describeError";
import { LocalArtifactScope, RichMarkdown } from "@codeterm/chat-ui";

export function NotificationBody({ text, tabId, failure = false, operation, preview = false }: { text: string; tabId: string | null; failure?: boolean; operation?: string; preview?: boolean }) {
  if (failure) {
    const description = describeError(text, { operation: operation ? `handling ${operation}` : "handling this notification" });
    return preview ? <span>{description.title}</span> : <InlineErrorNotice description={description} />;
  }
  return tabId
    ? <LocalArtifactScope tabId={tabId}><RichMarkdown text={text} /></LocalArtifactScope>
    : <RichMarkdown text={text} localArtifacts={false} />;
}
