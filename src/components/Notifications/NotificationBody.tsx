import { LocalArtifactScope, RichMarkdown } from "@codeterm/chat-ui";

export function NotificationBody({ text, tabId }: { text: string; tabId: string | null }) {
  return tabId
    ? <LocalArtifactScope tabId={tabId}><RichMarkdown text={text} /></LocalArtifactScope>
    : <RichMarkdown text={text} localArtifacts={false} />;
}
