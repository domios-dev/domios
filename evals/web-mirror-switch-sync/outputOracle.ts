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

/**
 * Process-lifetime, deliberately: `outputStream.ts` keeps a module-level
 * `replayed` set so a re-mounted tab does NOT re-request its history -- the
 * xterm widget still holds the scrollback. A per-run buffer would therefore
 * read empty on the second run and look like total output loss, which is a
 * property of the harness and not of the mirror. This models the scrollback
 * the widget keeps.
 */
const buffers = new Map<string, string[]>();

/**
 * Anchored containment. Markers ending in a number are prefixes of each other
 * -- WMSMARK-P2-1 sits inside WMSMARK-P2-10 -- so a bare `includes` would pass
 * a tab whose own output never arrived as long as a higher-numbered tab's did.
 * That is a false PASS on exactly the misrouting this oracle exists to catch.
 */
function containsMarker(haystack: string, marker: string): boolean {
  let from = 0;
  for (;;) {
    const at = haystack.indexOf(marker, from);
    if (at < 0) return false;
    const next = haystack[at + marker.length];
    if (next === undefined || next < "0" || next > "9") return true;
    from = at + 1;
  }
}

export async function createOutputOracle(): Promise<OutputOracle> {
  const { subscribeTabOutput } = await import("../../src/lib/outputStream");
  const unsubscribes: Array<() => void> = [];
  let truncations = 0;

  return {
    async attach(tabIds) {
      for (const tabId of tabIds) {
        if (!buffers.has(tabId)) buffers.set(tabId, []);
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
        if (!containsMarker(this.received(tabId), marker)) lost.push(tabId);
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
