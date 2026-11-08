import { updateSourceLabel, updateSourceMessage } from "../../lib/updatePanelModel";
import { InlineErrorNotice } from "./ErrorNotice";

export function UpdateChannelNotice({ channel, message }: { channel: string; message: string }) {
  return <InlineErrorNotice description={{
    title: `${updateSourceLabel(channel)} unavailable`,
    cause: updateSourceMessage(message),
    raw: message,
    actions: [],
  }} />;
}
