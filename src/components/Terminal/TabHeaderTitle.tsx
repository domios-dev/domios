import { Bot } from "lucide-react";
import { roleKindLabel } from "../../lib/roleKind";
import { localInstanceLabel } from "../../lib/localInstanceName";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { remoteMachineName } from "../../lib/remoteChatState";
import { useAccountMachineNames, useMeshMachineChoices, useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import type { Tab } from "../../lib/types";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { roleShieldLabel } from "../Sidebar/orchestrationSections";

export function TabHeaderTitle({ tab }: { tab: Tab }) {
  const peerId = parseCompositeTabId(tab.id)?.peerId ?? tab.remote_peer_id;
  const peerName = useMeshPeerName(peerId);
  const accountNames = useAccountMachineNames();
  const { local_instance_name, peers, installations } = useMeshMachineChoices();
  const hasOtherMachines = peers?.length || installations?.some(installation => installation.active) || accountNames.peers.size;
  const machine = peerId
    ? remoteMachineName(peerId, peerName, tab.remote_peer_name)
    : hasOtherMachines ? accountNames.local ?? local_instance_name ?? localInstanceLabel() : null;
  const name = displayTitleForTab(tab);
  const role = roleShieldLabel(tab) ?? roleKindLabel(tab) ?? "Agent";

  return (
    <div className="ct-tab-header-title">
      <Bot className="ct-tab-header-title__glyph" size={14} aria-hidden="true" />
      <span className="ct-tab-header-title__name" title={name}>{name}</span>
      <span className="ct-tab-header-title__subtitle">{machine ? `${role} · ${machine}` : role}</span>
    </div>
  );
}
