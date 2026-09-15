import { invoke } from "../../lib/bridge";
import { quotePathForPaste } from "../../lib/clipboardPaste";
import { IS_WEB, relayTarget } from "../../lib/webApi";
import type { AttachmentHandoffDeps, TransferredAttachment } from "./meshAttachmentHandoff";

export const composerAttachmentHandoff: AttachmentHandoffDeps = {
  relayPeerId: () => (IS_WEB ? relayTarget()?.id ?? null : null),
  transfer: (request) => invoke<TransferredAttachment>("mesh_transfer_attachment", request),
  transcribeLocal: (path) => invoke<string>("transcribe_voice_attachment", { path }),
  quote: quotePathForPaste,
};
