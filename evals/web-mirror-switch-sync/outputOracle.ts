/**
 * The per-tab output oracle.
 *
 * The mux oracle compares the tab list, focus and titles. Andrey's words were
 * "loses sync with the chat UI", which is the content behind a tab, not the
 * list of them. This subscribes through the product's own
 * `subscribeTabOutput` and asks a different question: after a switch storm,
 * does every tab's output that the daemon produced actually reach the client?
 *
 * No PTY input is written. Each tab emits its marker from its own startup
 * command, so this exercises the output path without touching the PTY input
 * delivery path, which is fragile and gated on explicit approval.
 *
 * The switch storm is the pressure: a tab's first subscription this page
 * lifetime sets its offset to 0 and forces the single shared multiplexed
 * stream to reconnect (`outputStream.ts` subscribeTabOutput/forceReconnect),
 * so rapid switching tears that one connection down repeatedly while every
 * other tab is still streaming through it.
 */

export interface OutputOracle {
  /** Attach to every tab, as a browser with them all mounted would. */
  attach(tabIds: string[]): Promise<void>;
  /** Everything the client received for a tab this run. */
  received(tabId: string): string;
  /** Tabs whose marker never arrived. */
  missing(markers: Map<string, string>): string[];
  truncations(): number;
  detach(): void;
}

export async function createOutputOracle(): Promise<OutputOracle> {
  const { subscribeTabOutput } = await import("../../src/lib/outputStream");
  const buffers = new Map<string, string[]>();
  const unsubscribes: Array<() => void> = [];
  let truncations = 0;

  return {
    async attach(tabIds) {
      for (const tabId of tabIds) {
        buffers.set(tabId, []);
        unsubscribes.push(
          subscribeTabOutput(tabId, {
            onOutput(text) {
              buffers.get(tabId)?.push(text);
            },
            onTruncated() {
              truncations++;
            },
            onEnded() {},
          }),
        );
      }
    },
    received(tabId) {
      return (buffers.get(tabId) ?? []).join("");
    },
    missing(markers) {
      const lost: string[] = [];
      for (const [tabId, marker] of markers) {
        if (!this.received(tabId).includes(marker)) lost.push(tabId);
      }
      return lost;
    },
    truncations: () => truncations,
    detach() {
      for (const unsubscribe of unsubscribes) unsubscribe();
      unsubscribes.length = 0;
    },
  };
}
