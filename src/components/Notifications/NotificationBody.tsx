import { InlineErrorNotice } from "../shared/ErrorNotice";
import { describeError } from "../../lib/describeError";
import { DsText, LocalArtifactScope } from "@codeterm/chat-ui";

export function NotificationBody({ text, tabId, failure = false, operation, preview = false }: { text: string; tabId: string | null; failure?: boolean; operation?: string; preview?: boolean }) {
  if (failure) {
    const description = describeError(text);
    return preview ? <span>{description.title === operation ? description.cause : description.title}</span> : <InlineErrorNotice description={description} />;
  }
  if (preview) return <span>{text}</span>;
  return tabId
    ? <LocalArtifactScope tabId={tabId}><DsText text={text} renderText={source => <span className="whitespace-pre-wrap">{source}</span>} /></LocalArtifactScope>
    : <span className="whitespace-pre-wrap">{text}</span>;
}
