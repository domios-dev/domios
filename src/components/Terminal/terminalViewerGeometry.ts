import type { Tab } from "../../lib/types";
import { tabOutputRoute } from "../../lib/remoteTabRoute";

export function terminalViewerLocalGeometry(
  tab: Pick<Tab, "id"> | null | undefined,
  relayPeerId: string | null,
): boolean {
  return tab !== null && tab !== undefined && tabOutputRoute(tab.id, relayPeerId) !== null;
}
