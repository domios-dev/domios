import type { ComponentProps } from "react";
import { MeshCanvas } from "./MeshCanvas";
import { useCanvasAccount } from "./useCanvasAccount";

type Props = Omit<ComponentProps<typeof MeshCanvas>, "account"> & { onAccountChanged: () => void };

/** The Mesh canvas with the account's machines on it; mounted only while the canvas is shown. */
export function AccountMeshCanvas({ onAccountChanged, ...props }: Props) {
  const account = useCanvasAccount(props.peers, onAccountChanged);
  return <MeshCanvas {...props} account={account} />;
}
