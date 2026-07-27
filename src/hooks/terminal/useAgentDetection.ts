import type { Terminal } from "@xterm/xterm";

export interface UseAgentDetectionOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  tabId: string;
  termReady: boolean;
}

// Tab titles are owned by spawn/rename paths only — not passive OSC updates.
export function useAgentDetection(_options: UseAgentDetectionOptions): void {}
