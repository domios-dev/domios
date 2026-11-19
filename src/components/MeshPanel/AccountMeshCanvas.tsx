import { useMemo, type ComponentProps } from "react";
import { MeshCanvas } from "./MeshCanvas";
import type { MeshAccount } from "./useMeshAccount";
import { useCanvasAccount } from "./useCanvasAccount";

type Props = Omit<ComponentProps<typeof MeshCanvas>, "account"> & { meshAccount: MeshAccount; onAccountChanged: () => void; registryRefreshKey?: number };

/** The Mesh canvas with the account's machines on it; mounted only while the canvas is shown. */
export function AccountMeshCanvas({ meshAccount, onAccountChanged, registryRefreshKey = 0, ...props }: Props) {
  const account = useCanvasAccount(props.peers, onAccountChanged, registryRefreshKey, meshAccount);
  const peers = useMemo(() => props.peers.filter(peer => account.canvas.peerFooters.has(peer.instance_id)), [props.peers, account.canvas.peerFooters]);
  return <MeshCanvas {...props} peers={peers} account={account} />;
}
