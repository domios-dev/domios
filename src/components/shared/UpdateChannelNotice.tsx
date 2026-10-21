import { InlineErrorNotice } from "./ErrorNotice";

export function UpdateChannelNotice({ channel, message }: { channel: string; message: string }) {
  return <InlineErrorNotice description={{
    title: `${channel} updates unavailable`,
    cause: "Couldn't refresh this channel. Review the details, then check again.",
    raw: message,
    actions: [],
  }} />;
}
