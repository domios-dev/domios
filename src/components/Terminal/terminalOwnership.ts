import { IS_WEB } from "../../lib/webApi";

export function terminalOwnsLivePty(args: {
  isWeb?: boolean;
  terminalVisible: boolean;
  tabVisible: boolean;
  ptyId?: string | null;
}): boolean {
  const isWeb = args.isWeb ?? IS_WEB;
  return (!isWeb || args.terminalVisible) && (args.tabVisible || !!args.ptyId);
}

export function terminalRenderEnabled(args: {
  terminalVisible: boolean;
  tabVisible: boolean;
}): boolean {
  return args.tabVisible && args.terminalVisible;
}
