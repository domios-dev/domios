import { IS_WEB } from "../../lib/webApi";

export function terminalOwnsLivePty(args: {
  isWeb?: boolean;
  terminalVisible: boolean;
  paneVisible: boolean;
  ptyId?: string | null;
}): boolean {
  const isWeb = args.isWeb ?? IS_WEB;
  return (!isWeb || args.terminalVisible) && (args.paneVisible || !!args.ptyId);
}

export function terminalRenderEnabled(args: {
  terminalVisible: boolean;
  paneVisible: boolean;
}): boolean {
  return args.paneVisible && args.terminalVisible;
}
