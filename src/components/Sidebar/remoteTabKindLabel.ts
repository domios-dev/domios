import { roleKindLabel } from "../../lib/roleKind";
import { resolveTabProvider } from "../../lib/tabProvider";
import type { Tab } from "../../lib/types";

export function remoteTabKindLabel(tab: Tab): string {
  const role = roleKindLabel(tab);
  if (role) return role;
  const provider = tab.agent_type || tab.backing?.kind === "chat_backend" ? resolveTabProvider(tab) : null;
  return provider ? `Agent: ${provider.displayName}` : "Shell";
}
