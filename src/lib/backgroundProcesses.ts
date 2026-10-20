export interface BackgroundProcessAuthority {
  rootThreadId: string;
  bindingNonce: string;
  epoch: number;
}

export interface BackgroundProcessEntry {
  threadId: string;
  processId: string;
  itemId: string;
  command: string;
  cwd: string;
  osPid: number | null;
}

export interface BackgroundProcessSnapshot extends BackgroundProcessAuthority {
  processes: BackgroundProcessEntry[];
}

export function backgroundProcessesSuffix(authority: BackgroundProcessAuthority): string {
  const query = new URLSearchParams({
    rootThreadId: authority.rootThreadId,
    bindingNonce: authority.bindingNonce,
    epoch: String(authority.epoch),
  });
  return `/agent/runtime/background-processes?${query}`;
}
