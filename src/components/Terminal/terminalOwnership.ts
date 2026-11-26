import { IS_WEB } from "../../lib/webApi";

export function terminalOwnsLivePty(args: {
  isWeb?: boolean;
  terminalVisible: boolean;
  tabVisible: boolean;
  ptyId?: string | null;
  frozen?: boolean;
  chatBackend?: boolean;
}): boolean {
  if (args.frozen || args.chatBackend) return false;
  const isWeb = args.isWeb ?? IS_WEB;
  return (!isWeb || args.terminalVisible) && (args.tabVisible || !!args.ptyId);
}

export function terminalRenderEnabled(args: {
  terminalVisible: boolean;
  tabVisible: boolean;
  chatBackend?: boolean;
}): boolean {
  return !args.chatBackend && args.tabVisible && args.terminalVisible;
}
