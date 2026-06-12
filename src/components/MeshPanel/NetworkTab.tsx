import { useCallback, useState } from "react";
import { invoke } from "../../lib/bridge";
import { Radio, Search, Users } from "lucide-react";
import { BroadcastToggle } from "./BroadcastToggle";
import AddPeerPanel from "./AddPeerPanel";
import { SettingsPage, SettingsCard, SettingsPill, SettingsRow } from "../Settings/shared";
import type { MeshData } from "./useMeshData";

interface Props {
  data: MeshData;
  triggerRefresh: () => void;
}

export function NetworkTab({ data, triggerRefresh }: Props) {
  const [exiting, setExiting] = useState(false);

  const handleForget = useCallback(async (instanceId: string) => {
    try {
      await invoke("mesh_disconnect", { instanceId });
      triggerRefresh();
    } catch (e) {
      console.error("Forget peer failed:", e);
    }
  }, [triggerRefresh]);

  const handleExit = useCallback(async () => {
    setExiting(true);
    try {
      await invoke("mesh_exit");
      triggerRefresh();
    } catch (e) {
      console.error("Leave mesh failed:", e);
    } finally {
      setExiting(false);
    }
  }, [triggerRefresh]);

  return (
    <div className="h-full overflow-y-auto">
      <div className="p-6">
        <SettingsPage
          title="Connect your machines"
          subtitle="Discover, pair, and share workspaces across devices on your network or Tailscale."
        >
          <SettingsCard
            icon={<Radio size={16} />}
            title="Broadcast presence"
            headerRight={<BroadcastToggle />}
            description="Make this machine visible to other CodeTerm instances on your local network for 5 minutes. Other machines can then send a pair request. Turn this on briefly when adding a new device."
          />

          <SettingsCard
            icon={<Search size={16} />}
            title="Find peers"
            description="Discover machines that are currently broadcasting, or paste an address manually."
          >
            <AddPeerPanel />
          </SettingsCard>

          {data.peers.length > 0 && (
            <SettingsCard
              icon={<Users size={16} />}
              title={`Paired devices · ${data.peers.length}`}
              headerRight={
                <button
                  onClick={handleExit}
                  disabled={exiting}
                  title="Disconnect from all peers and stop participating in the mesh"
                  className="text-[10px] uppercase tracking-wider px-2.5 py-1 rounded border border-[var(--ct-red)]/30 text-[var(--ct-red)]/70 hover:text-[var(--ct-red)] hover:border-[var(--ct-red)] hover:bg-[var(--ct-red)]/10 transition-colors disabled:opacity-30 cursor-pointer bg-transparent"
                >
                  {exiting ? "Leaving…" : "Leave mesh"}
                </button>
              }
            >
              <ul className="divide-y divide-[var(--ct-border)] rounded border border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)]">
                {data.peers.map((peer) => (
                  <li
                    key={peer.instance_id}
                    data-testid="mesh-peer"
                    data-peer-id={peer.instance_id}
                    data-online={String(!!peer.online)}
                    data-pending={String(!!peer.pending)}
                    className="p-3"
                  >
                    <SettingsRow>
                      <div className="flex items-center gap-2 min-w-0">
                        <div className={`w-2 h-2 rounded-full flex-shrink-0 ${
                          peer.pending
                            ? "border border-dashed border-[var(--ct-muted)] bg-transparent"
                            : peer.online
                              ? "bg-[var(--ct-green)]"
                              : "bg-[var(--ct-muted)]"
                        }`} />
                        <span className={`text-xs font-medium truncate ${
                          peer.pending ? "text-[var(--ct-muted-fg)]" : "text-[var(--ct-fg)]"
                        }`}>{peer.name}</span>
                        <span className="text-[10px] text-[var(--ct-muted)] font-mono flex-shrink-0">{peer.ip}</span>
                        {peer.pending ? (
                          <SettingsPill tone="muted">auto-pairs when online</SettingsPill>
                        ) : (
                          <SettingsPill tone="muted">{peer.access_level}</SettingsPill>
                        )}
                      </div>
                      {peer.pending ? (
                        <span className="text-[10px] text-[var(--ct-muted)] italic flex-shrink-0">
                          {peer.vouched_by_names && peer.vouched_by_names.length > 0
                            ? `vouched by ${peer.vouched_by_names.join(", ")}`
                            : "waiting"}
                        </span>
                      ) : (
                        <button
                          onClick={() => handleForget(peer.instance_id)}
                          className="text-[10px] px-2 py-1 rounded text-[var(--ct-red)]/70 hover:text-[var(--ct-red)] hover:bg-[var(--ct-red)]/10 transition-colors flex-shrink-0"
                        >
                          Forget
                        </button>
                      )}
                    </SettingsRow>
                  </li>
                ))}
              </ul>
            </SettingsCard>
          )}
        </SettingsPage>
      </div>
    </div>
  );
}
